use std::collections::HashSet;

use crate::config::SeatConfig;
use crate::models::{SeatMap, SeatRow, SelectedSeat};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// "A" → 0, "B" → 1, … "Z" → 25.  Uses the first character of the code.
fn row_to_index(code: &str) -> usize {
    code.chars()
        .next()
        .map(|c| (c.to_ascii_uppercase() as usize).saturating_sub('A' as usize))
        .unwrap_or(0)
}

/// Parse the column number from a seat label like "D6" or "A12".
fn seat_col(label: &str) -> Option<usize> {
    let digits: String = label.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Find `quantity` consecutive available seats in `sm` whose center column is
/// closest to the row's midpoint.  Returns `None` if not enough seats exist.
///
/// "Consecutive" means adjacent column numbers AND the same `grid_cd` section
/// (so seats on opposite sides of a centre aisle are never grouped together).
fn best_consecutive(sm: &SeatMap, quantity: usize) -> Option<Vec<SelectedSeat>> {
    // Build col → &SeatRow for availability checking and booking-ID lookup.
    let seat_by_col: std::collections::HashMap<usize, &SeatRow> = sm
        .seat_rows
        .iter()
        .filter_map(|sr| seat_col(&sr.seat_row).map(|col| (col, sr)))
        .collect();

    let mut available: Vec<usize> = seat_by_col
        .iter()
        .filter(|(_, sr)| sr.status == 1)
        .map(|(col, _)| *col)
        .collect();
    available.sort_unstable();

    if available.len() < quantity {
        return None;
    }

    let center = sm.max_row.saturating_add(1) / 2; // 1-based midpoint
    let mut best: Option<(Vec<usize>, usize)> = None; // (group, dist)

    for window in available.windows(quantity) {
        // Seats must be numerically consecutive AND on the same side of any aisle.
        let all_consecutive = window.windows(2).all(|w| {
            if w[1] != w[0] + 1 {
                return false;
            }
            // If either seat has an aisle-side tag, they must match.
            let side_a = seat_by_col.get(&w[0]).and_then(|sr| sr.aisle_side.as_deref());
            let side_b = seat_by_col.get(&w[1]).and_then(|sr| sr.aisle_side.as_deref());
            match (side_a, side_b) {
                (Some(a), Some(b)) => a == b,
                _ => true, // no aisle info → assume same side (layout has no centre aisle)
            }
        });
        if all_consecutive {
            let group_center = (window[0] + window[window.len() - 1] + 1) / 2;
            let dist = (group_center as isize - center as isize).unsigned_abs();
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                best = Some((window.to_vec(), dist));
            }
        }
    }

    best.map(|(cols, _)| {
        cols.iter()
            .map(|col| {
                let sr = seat_by_col[col];
                SelectedSeat {
                    seat_id: sr
                        .booking_id
                        .clone()
                        .unwrap_or_else(|| format!("{}{}", sm.seat_code, col)),
                    display: sr.seat_row.clone(),
                    grd_cd: sr.grid_cd.clone().unwrap_or_default(),
                }
            })
            .collect()
    })
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Select `config.quantity` seats from `seat_map`.
///
/// Row orientation (tix.id): A = back (far from screen), larger letters = closer to screen.
///
/// Algorithm:
/// 1. If `manual_seats` are all available → return them.
/// 2. Auto-select:
///    a. Skip the last `avoid_first_rows` rows (front = closest to screen = largest letters).
///    b. Among remaining rows, try `preferred_rows` range first (center-out order).
///    c. Fall back to other rows if the preferred range has no group.
pub fn select(seat_map: &[SeatMap], config: &SeatConfig) -> Option<Vec<SelectedSeat>> {
    // Precompute available seat labels once — O(total_seats) — so all
    // subsequent lookups are O(1) instead of O(rows × seats_per_row).
    let seat_row_lookup: std::collections::HashMap<&str, &SeatRow> = seat_map
        .iter()
        .flat_map(|sm| sm.seat_rows.iter())
        .filter(|sr| sr.status == 1)
        .map(|sr| (sr.seat_row.as_str(), sr))
        .collect();
    let available_set: HashSet<&str> = seat_row_lookup.keys().copied().collect();

    // 1. Manual seats
    if !config.manual_seats.is_empty() {
        let all_ok = config
            .manual_seats
            .iter()
            .all(|wanted| available_set.contains(wanted.as_str()));
        if all_ok {
            return Some(
                config
                    .manual_seats
                    .iter()
                    .map(|s| {
                        let sr = seat_row_lookup[s.as_str()];
                        SelectedSeat {
                            seat_id: sr.booking_id.clone().unwrap_or_else(|| s.clone()),
                            display: s.clone(),
                            grd_cd: sr.grid_cd.clone().unwrap_or_default(),
                        }
                    })
                    .collect(),
            );
        }
    }

    // 2. Collect usable rows:
    //    - Skip the last `avoid_first_rows` rows (front = closest to screen = largest letters).
    //      In tix.id: A = back (far from screen), Z/Y/X = front (close to screen).
    //      We find the max row index in this layout and exclude the top N.
    let max_row_idx = seat_map
        .iter()
        .map(|sm| row_to_index(&sm.seat_code))
        .max()
        .unwrap_or(0);

    let mut usable: Vec<&SeatMap> = seat_map
        .iter()
        .filter(|sm| {
            let idx = row_to_index(&sm.seat_code);
            // max_row_idx - idx is the "distance from front": 0 = front, max = back.
            // Keep only rows at least avoid_first_rows away from the front.
            max_row_idx.saturating_sub(idx) >= config.avoid_first_rows
                && sm
                    .seat_rows
                    .iter()
                    .any(|sr| available_set.contains(sr.seat_row.as_str()))
        })
        .collect();
    usable.sort_by_key(|sm| row_to_index(&sm.seat_code));

    // Preferred row index range — normalise so lo ≤ hi regardless of config order.
    let (pref_lo, pref_hi) = if config.preferred_rows.len() == 2 {
        let a = row_to_index(&config.preferred_rows[0]);
        let b = row_to_index(&config.preferred_rows[1]);
        (a.min(b), a.max(b))
    } else {
        (0, usize::MAX)
    };

    let preferred: Vec<&SeatMap> = usable
        .iter()
        .filter(|sm| {
            let i = row_to_index(&sm.seat_code);
            i >= pref_lo && i <= pref_hi
        })
        .copied()
        .collect();

    // Order preferred rows center-out within the range
    let range_center = if pref_hi != usize::MAX {
        (pref_lo + pref_hi) / 2
    } else if !preferred.is_empty() {
        let lo = row_to_index(&preferred[0].seat_code);
        let hi = row_to_index(&preferred[preferred.len() - 1].seat_code);
        (lo + hi) / 2
    } else {
        0
    };

    let mut ordered = preferred.clone();
    ordered.sort_by_key(|sm| {
        let i = row_to_index(&sm.seat_code) as isize;
        (i - range_center as isize).abs()
    });

    let fallback: Vec<&SeatMap> = usable
        .iter()
        .filter(|sm| {
            let i = row_to_index(&sm.seat_code);
            i < pref_lo || i > pref_hi
        })
        .copied()
        .collect();

    // Try preferred (center-out) then fallback rows in order
    for sm in ordered.iter().chain(fallback.iter()) {
        if let Some(seats) = best_consecutive(sm, config.quantity) {
            return Some(seats);
        }
    }

    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SeatConfig;
    use crate::models::{SeatMap, SeatRow};

    fn make_sr(label: &str, status: i32, booking_id: Option<&str>, grid_cd: Option<&str>, aisle_side: Option<&str>) -> SeatRow {
        SeatRow {
            seat_row: label.to_string(),
            booking_id: booking_id.map(|s| s.to_string()),
            status,
            grid_cd: grid_cd.map(|s| s.to_string()),
            aisle_side: aisle_side.map(|s| s.to_string()),
        }
    }

    fn make_sm(code: &str, max_row: usize, rows: Vec<SeatRow>) -> SeatMap {
        SeatMap { seat_code: code.to_string(), max_row, seat_rows: rows }
    }

    fn cfg(qty: usize, manual: Vec<&str>, avoid: usize, preferred: Vec<&str>) -> SeatConfig {
        SeatConfig {
            quantity: qty,
            manual_seats: manual.into_iter().map(|s| s.to_string()).collect(),
            avoid_first_rows: avoid,
            preferred_rows: preferred.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    // ── row_to_index ──────────────────────────────────────────────────────

    #[test] fn row_to_index_a_is_0() { assert_eq!(row_to_index("A"), 0); }
    #[test] fn row_to_index_b_is_1() { assert_eq!(row_to_index("B"), 1); }
    #[test] fn row_to_index_z_is_25() { assert_eq!(row_to_index("Z"), 25); }
    #[test] fn row_to_index_lowercase() { assert_eq!(row_to_index("c"), 2); }
    #[test] fn row_to_index_empty_is_0() { assert_eq!(row_to_index(""), 0); }

    // ── seat_col ──────────────────────────────────────────────────────────

    #[test] fn seat_col_single_digit() { assert_eq!(seat_col("A6"), Some(6)); }
    #[test] fn seat_col_double_digit() { assert_eq!(seat_col("A12"), Some(12)); }
    #[test] fn seat_col_no_digit() { assert_eq!(seat_col("A"), None); }
    #[test] fn seat_col_empty_string() { assert_eq!(seat_col(""), None); }

    // ── select: manual seats ──────────────────────────────────────────────

    #[test]
    fn select_manual_all_available() {
        let map = vec![make_sm("A", 5, vec![
            make_sr("A1", 1, None, None, None),
            make_sr("A2", 1, None, None, None),
            make_sr("A3", 0, None, None, None),
        ])];
        let result = select(&map, &cfg(2, vec!["A1", "A2"], 0, vec![])).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].display, "A1");
        assert_eq!(result[1].display, "A2");
    }

    #[test]
    fn select_manual_one_unavailable_falls_through_to_auto() {
        // A2 is unavailable → manual check fails → auto-select finds A3+A4
        let map = vec![make_sm("A", 5, vec![
            make_sr("A1", 1, None, None, None),
            make_sr("A2", 0, None, None, None),
            make_sr("A3", 1, None, None, None),
            make_sr("A4", 1, None, None, None),
        ])];
        let result = select(&map, &cfg(2, vec!["A1", "A2"], 0, vec![])).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn select_manual_uses_booking_id_and_grid_cd() {
        let map = vec![make_sm("A", 5, vec![
            make_sr("A1", 1, Some("bk-001"), Some("GRD01"), None),
            make_sr("A2", 1, Some("bk-002"), Some("GRD01"), None),
        ])];
        let result = select(&map, &cfg(2, vec!["A1", "A2"], 0, vec![])).unwrap();
        assert_eq!(result[0].seat_id, "bk-001");
        assert_eq!(result[0].grd_cd, "GRD01");
        assert_eq!(result[1].seat_id, "bk-002");
    }

    // ── select: auto-select ───────────────────────────────────────────────

    #[test]
    fn select_auto_finds_consecutive_pair() {
        let map = vec![make_sm("A", 4, vec![
            make_sr("A1", 0, None, None, None),
            make_sr("A2", 1, None, None, None),
            make_sr("A3", 1, None, None, None),
            make_sr("A4", 0, None, None, None),
        ])];
        let result = select(&map, &cfg(2, vec![], 0, vec![])).unwrap();
        assert_eq!(result.len(), 2);
        let labels: Vec<_> = result.iter().map(|s| s.display.as_str()).collect();
        assert_eq!(labels, ["A2", "A3"]);
    }

    #[test]
    fn select_none_when_gap_between_available() {
        // A1 and A3 available but A2 is not — no 2 consecutive
        let map = vec![make_sm("A", 3, vec![
            make_sr("A1", 1, None, None, None),
            make_sr("A2", 0, None, None, None),
            make_sr("A3", 1, None, None, None),
        ])];
        assert!(select(&map, &cfg(2, vec![], 0, vec![])).is_none());
    }

    #[test]
    fn select_none_on_empty_map() {
        assert!(select(&[], &cfg(1, vec![], 0, vec![])).is_none());
    }

    #[test]
    fn select_single_seat() {
        let map = vec![make_sm("D", 10, vec![make_sr("D5", 1, None, None, None)])];
        let result = select(&map, &cfg(1, vec![], 0, vec![])).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].display, "D5");
    }

    #[test]
    fn select_seat_id_falls_back_to_code_plus_col() {
        // No booking_id → seat_id derived from seat_code + col number
        let map = vec![make_sm("A", 5, vec![make_sr("A3", 1, None, None, None)])];
        let result = select(&map, &cfg(1, vec![], 0, vec![])).unwrap();
        assert_eq!(result[0].seat_id, "A3");
    }

    // ── select: avoid_first_rows ──────────────────────────────────────────

    #[test]
    fn select_avoids_front_rows() {
        // A(idx=0)=back, B(idx=1)=front. avoid_first_rows=1 excludes B.
        let map = vec![
            make_sm("A", 5, vec![make_sr("A1", 1, None, None, None), make_sr("A2", 1, None, None, None)]),
            make_sm("B", 5, vec![make_sr("B1", 1, None, None, None), make_sr("B2", 1, None, None, None)]),
        ];
        let result = select(&map, &cfg(2, vec![], 1, vec![])).unwrap();
        assert!(result.iter().all(|s| s.display.starts_with('A')));
    }

    #[test]
    fn select_all_rows_excluded_by_avoid_returns_none() {
        // Only one row, avoid_first_rows=1 excludes it
        let map = vec![make_sm("A", 5, vec![make_sr("A1", 1, None, None, None), make_sr("A2", 1, None, None, None)])];
        assert!(select(&map, &cfg(2, vec![], 1, vec![])).is_none());
    }

    // ── select: preferred_rows ────────────────────────────────────────────

    #[test]
    fn select_preferred_row_range_takes_priority() {
        let map = vec![
            make_sm("A", 5, vec![make_sr("A1", 1, None, None, None), make_sr("A2", 1, None, None, None)]),
            make_sm("D", 5, vec![make_sr("D1", 1, None, None, None), make_sr("D2", 1, None, None, None)]),
            make_sm("H", 5, vec![make_sr("H1", 1, None, None, None), make_sr("H2", 1, None, None, None)]),
        ];
        let result = select(&map, &cfg(2, vec![], 0, vec!["D", "H"])).unwrap();
        // D is closest to center of D-H range
        let d = result.iter().all(|s| s.display.starts_with('D') || s.display.starts_with('H'));
        assert!(d, "Expected seats in D-H range, got: {:?}", result.iter().map(|s| &s.display).collect::<Vec<_>>());
    }

    #[test]
    fn select_falls_back_when_preferred_range_has_no_seats() {
        // preferred D-E, but D has no available seats → falls back to A
        let map = vec![
            make_sm("A", 5, vec![make_sr("A1", 1, None, None, None), make_sr("A2", 1, None, None, None)]),
            make_sm("D", 5, vec![make_sr("D1", 0, None, None, None)]),
        ];
        let result = select(&map, &cfg(2, vec![], 0, vec!["D", "E"])).unwrap();
        assert!(result.iter().all(|s| s.display.starts_with('A')));
    }

    // ── select: aisle-side blocking ───────────────────────────────────────

    #[test]
    fn select_aisle_side_prevents_cross_aisle_grouping() {
        // A1-A4 left side, A5-A8 right side. Asking for 3: no 3 same-side consecutive
        let map = vec![make_sm("A", 8, vec![
            make_sr("A1", 1, None, None, Some("L")),
            make_sr("A2", 1, None, None, Some("L")),
            make_sr("A3", 1, None, None, Some("R")),
            make_sr("A4", 1, None, None, Some("R")),
        ])];
        // 2-seat group is fine (A1+A2 or A3+A4)
        let result2 = select(&map, &cfg(2, vec![], 0, vec![]));
        assert!(result2.is_some());
        // 3-seat group cannot cross aisle
        let result3 = select(&map, &cfg(3, vec![], 0, vec![]));
        assert!(result3.is_none());
    }
}
