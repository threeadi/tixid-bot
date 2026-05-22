use std::collections::HashSet;

use crate::config::SeatConfig;
use crate::models::SeatMap;

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
fn best_consecutive(sm: &SeatMap, quantity: usize) -> Option<Vec<String>> {
    let available: HashSet<usize> = sm
        .seat_rows
        .iter()
        .filter(|sr| sr.status == 1)
        .filter_map(|sr| seat_col(&sr.seat_row))
        .collect();

    if available.len() < quantity {
        return None;
    }

    let mut cols: Vec<usize> = available.into_iter().collect();
    cols.sort_unstable();

    let center = sm.max_row.saturating_add(1) / 2; // 1-based midpoint
    let mut best: Option<(Vec<usize>, usize)> = None; // (group, dist)

    for window in cols.windows(quantity) {
        // Check all seats in the window are consecutive (+1 each step)
        if window.windows(2).all(|w| w[1] == w[0] + 1) {
            let group_center = (window[0] + window[window.len() - 1] + 1) / 2;
            let dist = (group_center as isize - center as isize).unsigned_abs();
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                best = Some((window.to_vec(), dist));
            }
        }
    }

    best.map(|(cols, _)| {
        cols.iter()
            .map(|col| format!("{}{}", sm.seat_code, col))
            .collect()
    })
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Select `config.quantity` seats from `seat_map`.
///
/// Algorithm:
/// 1. If `manual_seats` are all available → return them.
/// 2. Auto-select:
///    a. Skip the first `avoid_first_rows` rows (A, B, C, …).
///    b. Among remaining rows, try `preferred_rows` range first (center-out order).
///    c. Fall back to other rows if the preferred range has no group.
pub fn select(seat_map: &[SeatMap], config: &SeatConfig) -> Option<Vec<String>> {
    // Precompute available seat labels once — O(total_seats) — so all
    // subsequent lookups are O(1) instead of O(rows × seats_per_row).
    let available_set: HashSet<&str> = seat_map
        .iter()
        .flat_map(|sm| sm.seat_rows.iter())
        .filter(|sr| sr.status == 1)
        .map(|sr| sr.seat_row.as_str())
        .collect();

    // 1. Manual seats
    if !config.manual_seats.is_empty() {
        let all_ok = config
            .manual_seats
            .iter()
            .all(|wanted| available_set.contains(wanted.as_str()));
        if all_ok {
            return Some(config.manual_seats.clone());
        }
    }

    // 2. Collect usable rows (skip first N, must have at least one available seat)
    let mut usable: Vec<&SeatMap> = seat_map
        .iter()
        .filter(|sm| {
            row_to_index(&sm.seat_code) >= config.avoid_first_rows
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
