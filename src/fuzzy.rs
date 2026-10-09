//! Fuzzy matching and string normalization utilities for movie titles.

/// Normalizes a string by converting to lowercase, replacing non-alphanumeric
/// characters with spaces, and trimming/collapsing redundant whitespace.
pub fn normalize(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = true; // prevent leading space

    for c in s.chars() {
        if c.is_alphanumeric() {
            for lower in c.to_lowercase() {
                result.push(lower);
            }
            prev_space = false;
        } else if !prev_space {
            result.push(' ');
            prev_space = true;
        }
    }

    if result.ends_with(' ') {
        result.pop();
    }
    result
}

/// Tokenizes a normalized string into words.
pub fn tokenize(s: &str) -> Vec<String> {
    normalize(s)
        .split_whitespace()
        .map(|w| w.to_string())
        .collect()
}

/// Calculates token overlap ratio between target and candidate.
/// Returns a value in `[0.0, 1.0]`.
/// Supports fuzzy token matching (e.g. "avenger" matches "avengers").
pub fn token_overlap_score(target: &str, candidate: &str) -> f64 {
    let target_tokens = tokenize(target);
    let cand_tokens = tokenize(candidate);

    if target_tokens.is_empty() || cand_tokens.is_empty() {
        return 0.0;
    }

    let mut matched_target = 0usize;
    for t in &target_tokens {
        let is_matched = cand_tokens.iter().any(|c| {
            if t == c {
                return true;
            }
            // Check prefix matching (e.g., "avenger" prefix of "avengers")
            if (t.len() >= 4 && c.starts_with(t)) || (c.len() >= 4 && t.starts_with(c)) {
                return true;
            }
            // Levenshtein distance <= 1 for words >= 5 chars
            if t.len() >= 5 && c.len() >= 5 && levenshtein(t, c) <= 1 {
                return true;
            }
            false
        });
        if is_matched {
            matched_target += 1;
        }
    }

    matched_target as f64 / target_tokens.len() as f64
}

/// Standard Levenshtein distance between two strings.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev_row: Vec<usize> = (0..=n).collect();
    let mut curr_row: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr_row[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr_row[j] = (prev_row[j] + 1)
                .min(curr_row[j - 1] + 1)
                .min(prev_row[j - 1] + cost);
        }
        prev_row.copy_from_slice(&curr_row);
    }

    prev_row[n]
}

/// Normalized Levenshtein similarity score in `[0.0, 1.0]`.
pub fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    let norm_a = normalize(a);
    let norm_b = normalize(b);

    if norm_a == norm_b {
        return 1.0;
    }
    let max_len = norm_a.len().max(norm_b.len());
    if max_len == 0 {
        return 1.0;
    }

    let dist = levenshtein(&norm_a, &norm_b);
    1.0 - (dist as f64 / max_len as f64)
}

/// Composite similarity score combining token containment and character-level similarity.
/// Returns a value in `[0.0, 1.0]`.
pub fn calculate_similarity(target: &str, candidate: &str) -> f64 {
    let norm_target = normalize(target);
    let norm_cand = normalize(candidate);

    if norm_target.is_empty() || norm_cand.is_empty() {
        return 0.0;
    }

    if norm_target == norm_cand {
        return 1.0;
    }

    let token_score = token_overlap_score(&norm_target, &norm_cand);
    let lev_score = levenshtein_similarity(&norm_target, &norm_cand);

    let base = (token_score * 0.7) + (lev_score * 0.3);

    if norm_cand.contains(&norm_target) {
        (base + 0.2).min(1.0)
    } else if token_score >= 1.0 {
        (base + 0.15).min(1.0)
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("Avengers: Doomsday!"), "avengers doomsday");
        assert_eq!(normalize("  hasut - 2026 "), "hasut 2026");
        assert_eq!(normalize("MEMBURU_PEMANGSA"), "memburu pemangsa");
    }

    #[test]
    fn test_token_overlap() {
        let score = token_overlap_score("avenger doomsday", "Avengers: Doomsday (IMAX 2D)");
        assert_eq!(score, 1.0); // both "avenger" (matches "avengers") and "doomsday" found

        let partial = token_overlap_score("avenger secret wars", "Avengers: Doomsday");
        assert!(partial > 0.0 && partial < 1.0);
    }

    #[test]
    fn test_calculate_similarity() {
        let score1 = calculate_similarity("avenger doomsday", "Avengers: Doomsday");
        assert!(score1 >= 0.95);

        let score2 = calculate_similarity("avenger doomsday", "Avengers: Doomsday IMAX");
        assert!(score2 >= 0.85);

        let score3 = calculate_similarity("avenger doomsday", "Hasut");
        assert!(score3 < 0.3);
    }
}
