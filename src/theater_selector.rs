use std::collections::HashSet;

use crate::config::{ShowtimeConfig, TheaterConfig};
use crate::models::{ShowTime, Theater};

#[derive(Clone)]
pub struct SelectedShowtime {
    pub theater: Theater,
    pub category: String,
    pub showtime: ShowTime,
    /// Slug used by the seat-layout endpoint: "xxi", "cgv", "cinepolis", …
    pub merchant_slug: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn merchant_name_to_slug(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("xxi") {
        "xxi".to_string()
    } else if lower.contains("cgv") {
        "cgv".to_string()
    } else if lower.contains("cinepolis") || lower.contains("cin\u{e9}polis") {
        "cinepolis".to_string()
    } else {
        lower
    }
}

/// Check whether a showtime falls within the preferred range.
///
/// Both `HH:MM` (time-only) and `YYYY-MM-DD HH:MM` (full datetime) are
/// supported in the config.  When the config value contains a date part,
/// `target_date` is used to build the full comparison string.
/// An empty string in the config means "no bound" (accepted unconditionally).
fn showtime_in_range(target_date: &str, display_time: &str, start: &str, end: &str) -> bool {
    let start = start.trim();
    let end = end.trim();
    if start.is_empty() && end.is_empty() {
        return true;
    }

    // Build the candidate datetime string (always "YYYY-MM-DD HH:MM")
    let candidate = format!("{} {}", target_date, display_time);

    // Normalise a config bound: "HH:MM" → "YYYY-MM-DD HH:MM" using target_date
    let expand = |s: &str| -> String {
        if s.len() == 5 && s.contains(':') {
            format!("{} {}", target_date, s)
        } else {
            s.to_string()
        }
    };

    let after_start = start.is_empty() || candidate >= expand(start);
    let before_end  = end.is_empty()   || candidate <= expand(end);
    after_start && before_end
}

/// Normalise a date/datetime string for range comparisons.
/// "YYYY-MM-DD"       → "YYYY-MM-DD 00:00" (start-of-day for range starts)
/// "YYYY-MM-DD HH:MM" → unchanged
fn norm_start(s: &str) -> String {
    if s.len() == 10 {
        format!("{} 00:00", s)
    } else {
        s.to_string()
    }
}

/// Normalise a range-end datetime string.
/// "YYYY-MM-DD"       → "YYYY-MM-DD 23:59"
/// "YYYY-MM-DD HH:MM" → unchanged
fn norm_end(s: &str) -> String {
    if s.len() == 10 {
        format!("{} 23:59", s)
    } else {
        s.to_string()
    }
}

/// Returns true if the given showtime (`target_date` + `display_time`) falls
/// inside any of the blocked datetime ranges.
fn showtime_is_blocked(
    target_date: &str,
    display_time: &str,
    blocked: &[Vec<String>],
) -> bool {
    let dt = format!("{} {}", target_date, display_time);
    blocked.iter().any(|range| {
        if range.len() == 2 {
            let start = norm_start(&range[0]);
            let end = norm_end(&range[1]);
            dt >= start && dt <= end
        } else {
            false
        }
    })
}

/// Return the first showtime that is open (status=1), within the time range,
/// and not in a blocked datetime range.
fn find_best_showtime(
    theater: &Theater,
    cfg: &ShowtimeConfig,
    target_date: &str,
    blocked: &[Vec<String>],
) -> Option<(String, ShowTime)> {
    for pg in &theater.price_groups {
        for st in &pg.show_time {
            if st.status == 1
                && showtime_in_range(
                    target_date,
                    &st.display_time,
                    &cfg.preferred_time_start,
                    &cfg.preferred_time_end,
                )
                && !showtime_is_blocked(target_date, &st.display_time, blocked)
            {
                return Some((pg.category.clone(), st.clone()));
            }
        }
    }
    None
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Return ALL valid (theater, showtime) pairs ordered by `theater_priority`.
///
/// Order:
/// 1. Priority theaters in config order (first entry = highest priority).
/// 2. Non-priority theaters (as fallback) in original list order.
///
/// Use this when you need to try multiple theaters (e.g., seat availability).
pub fn rank(
    theaters: &[Theater],
    theater_cfg: &TheaterConfig,
    showtime_cfg: &ShowtimeConfig,
    target_date: &str,
    blocked: &[Vec<String>],
) -> Vec<SelectedShowtime> {
    let mut results: Vec<SelectedShowtime> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let blocked_theaters: Vec<String> = theater_cfg
        .blocked_theaters
        .iter()
        .map(|s| s.to_lowercase())
        .collect();

    let is_theater_blocked = |name: &str| -> bool {
        let lower = name.to_lowercase();
        blocked_theaters.iter().any(|b| lower.contains(b.as_str()))
    };

    let build = |theater: &Theater, cat: String, st: ShowTime| SelectedShowtime {
        merchant_slug: merchant_name_to_slug(&theater.merchant.merchant_name),
        theater: theater.clone(),
        category: cat,
        showtime: st,
    };

    // Priority theaters first (in config order)
    for priority in &theater_cfg.theater_priority {
        let p = priority.to_lowercase();
        for theater in theaters {
            if is_theater_blocked(&theater.name) {
                continue;
            }
            if theater.name.to_lowercase().contains(&p) {
                if let Some((cat, st)) = find_best_showtime(theater, showtime_cfg, target_date, blocked) {
                    let key = format!("{}:{}", theater.id, st.id);
                    if seen.insert(key) {
                        results.push(build(theater, cat, st));
                    }
                }
            }
        }
    }

    // Fallback: remaining theaters not in priority list
    for theater in theaters {
        if is_theater_blocked(&theater.name) {
            continue;
        }
        if let Some((cat, st)) = find_best_showtime(theater, showtime_cfg, target_date, blocked) {
            let key = format!("{}:{}", theater.id, st.id);
            if seen.insert(key) {
                results.push(build(theater, cat, st));
            }
        }
    }

    results
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ShowtimeConfig, TheaterConfig};
    use crate::models::{Merchant, PriceGroup, ShowTime, Theater};

    fn st(id: &str, display: &str, status: i32) -> ShowTime {
        ShowTime {
            id: id.to_string(),
            time: 0,
            display_time: display.to_string(),
            studio: "Studio 1".to_string(),
            expired: 9_999_999_999_000,
            status,
            price: 75_000,
        }
    }

    fn th(id: &str, name: &str, merchant_name: &str, times: Vec<ShowTime>) -> Theater {
        Theater {
            id: id.to_string(),
            name: name.to_string(),
            merchant: Merchant {
                merchant_id: format!("m-{}", id),
                merchant_name: merchant_name.to_string(),
            },
            address: None,
            price_groups: vec![PriceGroup {
                category: "REGULAR".to_string(),
                low_price: 50_000,
                high_price: 75_000,
                price_string: "Rp50.000".to_string(),
                show_time: times,
            }],
        }
    }

    fn st_cfg(start: &str, end: &str) -> ShowtimeConfig {
        ShowtimeConfig { preferred_time_start: start.to_string(), preferred_time_end: end.to_string() }
    }

    fn th_cfg(priorities: &[&str], blocked: &[&str]) -> TheaterConfig {
        TheaterConfig {
            theater_priority: priorities.iter().map(|s| s.to_string()).collect(),
            blocked_theaters: blocked.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ── merchant_name_to_slug ─────────────────────────────────────────────

    #[test] fn slug_xxi() { assert_eq!(merchant_name_to_slug("Cinema XXI"), "xxi"); }
    #[test] fn slug_xxi_lowercase() { assert_eq!(merchant_name_to_slug("cinema xxi"), "xxi"); }
    #[test] fn slug_cgv() { assert_eq!(merchant_name_to_slug("CGV Grand"), "cgv"); }
    #[test] fn slug_cinepolis_ascii() { assert_eq!(merchant_name_to_slug("Cinepolis"), "cinepolis"); }
    #[test] fn slug_cinepolis_unicode() { assert_eq!(merchant_name_to_slug("Cin\u{e9}polis"), "cinepolis"); }
    #[test] fn slug_unknown_lowercased() { assert_eq!(merchant_name_to_slug("MyCinema"), "mycinema"); }

    // ── norm_start / norm_end ─────────────────────────────────────────────

    #[test] fn norm_start_date_only() { assert_eq!(norm_start("2025-06-01"), "2025-06-01 00:00"); }
    #[test] fn norm_start_datetime_unchanged() { assert_eq!(norm_start("2025-06-01 12:30"), "2025-06-01 12:30"); }
    #[test] fn norm_end_date_only() { assert_eq!(norm_end("2025-06-01"), "2025-06-01 23:59"); }
    #[test] fn norm_end_datetime_unchanged() { assert_eq!(norm_end("2025-06-01 18:00"), "2025-06-01 18:00"); }

    // ── showtime_in_range ─────────────────────────────────────────────────

    #[test]
    fn in_range_no_bounds_accepts_any() {
        assert!(showtime_in_range("2025-06-01", "10:00", "", ""));
    }

    #[test]
    fn in_range_only_start_bound() {
        assert!(showtime_in_range("2025-06-01", "14:00", "12:00", ""));
        assert!(!showtime_in_range("2025-06-01", "10:00", "12:00", ""));
    }

    #[test]
    fn in_range_only_end_bound() {
        assert!(showtime_in_range("2025-06-01", "14:00", "", "18:00"));
        assert!(!showtime_in_range("2025-06-01", "20:00", "", "18:00"));
    }

    #[test]
    fn in_range_within_hhmm_bounds() {
        assert!(showtime_in_range("2025-06-01", "14:30", "12:00", "18:00"));
    }

    #[test]
    fn not_in_range_before_start() {
        assert!(!showtime_in_range("2025-06-01", "10:00", "12:00", "18:00"));
    }

    #[test]
    fn not_in_range_after_end() {
        assert!(!showtime_in_range("2025-06-01", "20:00", "12:00", "18:00"));
    }

    #[test]
    fn in_range_full_datetime_config() {
        assert!(showtime_in_range("2025-06-01", "14:30", "2025-06-01 12:00", "2025-06-01 18:00"));
    }

    #[test]
    fn not_in_range_different_date_in_config() {
        assert!(!showtime_in_range("2025-06-02", "14:30", "2025-06-01 12:00", "2025-06-01 18:00"));
    }

    // ── showtime_is_blocked ───────────────────────────────────────────────

    #[test]
    fn blocked_inside_datetime_range() {
        let b = vec![vec!["2025-06-01 12:00".to_string(), "2025-06-01 16:00".to_string()]];
        assert!(showtime_is_blocked("2025-06-01", "14:00", &b));
    }

    #[test]
    fn not_blocked_outside_datetime_range() {
        let b = vec![vec!["2025-06-01 12:00".to_string(), "2025-06-01 16:00".to_string()]];
        assert!(!showtime_is_blocked("2025-06-01", "18:00", &b));
    }

    #[test]
    fn blocked_date_only_range() {
        let b = vec![vec!["2025-06-01".to_string(), "2025-06-01".to_string()]];
        assert!(showtime_is_blocked("2025-06-01", "14:00", &b));
        assert!(!showtime_is_blocked("2025-06-02", "14:00", &b));
    }

    #[test]
    fn not_blocked_empty_ranges() {
        assert!(!showtime_is_blocked("2025-06-01", "14:00", &[]));
    }

    #[test]
    fn blocked_malformed_range_ignored() {
        let b = vec![vec!["2025-06-01 12:00".to_string()]]; // only one element
        assert!(!showtime_is_blocked("2025-06-01", "14:00", &b));
    }

    // ── rank ──────────────────────────────────────────────────────────────

    #[test]
    fn rank_priority_theater_comes_first() {
        let theaters = vec![
            th("1", "CGV Grand", "CGV", vec![st("s1", "14:00", 1)]),
            th("2", "XXI Mall", "Cinema XXI", vec![st("s2", "15:00", 1)]),
        ];
        let result = rank(&theaters, &th_cfg(&["XXI"], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert!(!result.is_empty());
        assert_eq!(result[0].theater.name, "XXI Mall");
    }

    #[test]
    fn rank_blocked_theater_excluded() {
        let theaters = vec![
            th("1", "XXI Blocked", "Cinema XXI", vec![st("s1", "14:00", 1)]),
            th("2", "CGV Grand", "CGV", vec![st("s2", "15:00", 1)]),
        ];
        let result = rank(&theaters, &th_cfg(&[], &["blocked"]), &st_cfg("", ""), "2025-06-01", &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].theater.name, "CGV Grand");
    }

    #[test]
    fn rank_closed_showtime_not_included() {
        let theaters = vec![th("1", "XXI Mall", "Cinema XXI", vec![st("s1", "14:00", 0)])];
        let result = rank(&theaters, &th_cfg(&[], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_deduplicates_same_theater_across_multiple_priorities() {
        let theaters = vec![
            th("1", "XXI Premier Mall", "Cinema XXI", vec![st("s1", "14:00", 1)]),
        ];
        // Both "XXI" and "Premier" match same theater
        let result = rank(&theaters, &th_cfg(&["XXI", "Premier"], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn rank_fallback_theaters_included() {
        let theaters = vec![
            th("1", "NonPriority Cinema", "SomeBrand", vec![st("s1", "14:00", 1)]),
        ];
        let result = rank(&theaters, &th_cfg(&["XXI"], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].theater.name, "NonPriority Cinema");
    }

    #[test]
    fn rank_showtime_out_of_preferred_range_excluded() {
        let theaters = vec![th("1", "XXI Mall", "Cinema XXI", vec![st("s1", "10:00", 1)])];
        let result = rank(&theaters, &th_cfg(&[], &[]), &st_cfg("12:00", "22:00"), "2025-06-01", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_blocked_datetime_excludes_showtime() {
        let blocked = vec![vec!["2025-06-01 13:00".to_string(), "2025-06-01 15:00".to_string()]];
        let theaters = vec![th("1", "XXI Mall", "Cinema XXI", vec![st("s1", "14:00", 1)])];
        let result = rank(&theaters, &th_cfg(&[], &[]), &st_cfg("", ""), "2025-06-01", &blocked);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_merchant_slug_set_correctly() {
        let theaters = vec![th("1", "CGV Grand", "CGV", vec![st("s1", "14:00", 1)])];
        let result = rank(&theaters, &th_cfg(&[], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert_eq!(result[0].merchant_slug, "cgv");
    }

    #[test]
    fn rank_empty_theaters_returns_empty() {
        let result = rank(&[], &th_cfg(&["XXI"], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_selected_showtime_fields_populated() {
        let theaters = vec![th("t1", "XXI Mall", "Cinema XXI", vec![st("s1", "14:30", 1)])];
        let result = rank(&theaters, &th_cfg(&[], &[]), &st_cfg("", ""), "2025-06-01", &[]);
        assert_eq!(result.len(), 1);
        let sel = &result[0];
        assert_eq!(sel.theater.id, "t1");
        assert_eq!(sel.showtime.id, "s1");
        assert_eq!(sel.showtime.display_time, "14:30");
        assert_eq!(sel.category, "REGULAR");
        assert_eq!(sel.merchant_slug, "xxi");
    }

    #[test]
    fn rank_case_insensitive_blocked_theaters() {
        let theaters = vec![
            th("1", "CGV GRAND MALL", "CGV", vec![st("s1", "14:00", 1)]),
        ];
        let result = rank(&theaters, &th_cfg(&[], &["cgv grand"]), &st_cfg("", ""), "2025-06-01", &[]);
        assert!(result.is_empty());
    }
}