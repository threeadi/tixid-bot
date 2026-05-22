use crate::config::{ShowtimeConfig, TheaterConfig};
use crate::models::{ShowTime, Theater};

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

/// Parse "HH:MM" → total minutes, returns None on bad input.
fn hhmm_to_mins(s: &str) -> Option<u32> {
    let mut it = s.splitn(2, ':');
    let h: u32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    Some(h * 60 + m)
}

fn showtime_in_range(display_time: &str, start: &str, end: &str) -> bool {
    match (
        hhmm_to_mins(display_time),
        hhmm_to_mins(start),
        hhmm_to_mins(end),
    ) {
        (Some(t), Some(s), Some(e)) => t >= s && t <= e,
        // If any parse fails (e.g. empty string in config), accept all times.
        _ => true,
    }
}

/// Return the first showtime that is open (status=1) and within the time range.
fn find_best_showtime(
    theater: &Theater,
    cfg: &ShowtimeConfig,
) -> Option<(String, ShowTime)> {
    for pg in &theater.price_groups {
        for st in &pg.show_time {
            if st.status == 1
                && showtime_in_range(
                    &st.display_time,
                    &cfg.preferred_time_start,
                    &cfg.preferred_time_end,
                )
            {
                return Some((pg.category.clone(), st.clone()));
            }
        }
    }
    None
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Select the best (theater, showtime) pair based on `theater_priority`.
///
/// Algorithm:
/// 1. Iterate `theater_priority` top-to-bottom (substring match, case-insensitive).
/// 2. First matching theater that has a valid showtime wins.
/// 3. If none match → fallback to the first theater that has any valid showtime.
/// 4. If `theater_priority` is empty → go straight to fallback.
pub fn select(
    theaters: &[Theater],
    theater_cfg: &TheaterConfig,
    showtime_cfg: &ShowtimeConfig,
) -> Option<SelectedShowtime> {
    let build = |theater: &Theater, cat: String, st: ShowTime| SelectedShowtime {
        merchant_slug: merchant_name_to_slug(&theater.merchant.merchant_name),
        theater: theater.clone(),
        category: cat,
        showtime: st,
    };

    // Try each priority entry in order
    for priority in &theater_cfg.theater_priority {
        let p = priority.to_lowercase();
        for theater in theaters {
            if theater.name.to_lowercase().contains(&p) {
                if let Some((cat, st)) = find_best_showtime(theater, showtime_cfg) {
                    return Some(build(theater, cat, st));
                }
            }
        }
    }

    // Fallback: first theater with any valid showtime
    for theater in theaters {
        if let Some((cat, st)) = find_best_showtime(theater, showtime_cfg) {
            return Some(build(theater, cat, st));
        }
    }

    None
}
