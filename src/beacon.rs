//! Movie discovery (Beaconing / Reconnoitering) module.
//! Periodically scrapes the public TIX.ID catalog to locate upcoming or newly listed movies.

use anyhow::{Context, Result};
use reqwest::Client;
use std::collections::HashMap;

use crate::fuzzy;

/// Represents a candidate movie parsed from TIX.ID catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieCandidate {
    pub id: String,
    pub slug: String,
    pub display_name: String,
}

/// Minimum similarity threshold to accept a movie match (0.0 to 1.0).
pub const DEFAULT_MATCH_THRESHOLD: f64 = 0.65;

/// Fetches the public movie catalog from https://www.tix.id and extracts movie links.
pub async fn fetch_catalog(client: &Client) -> Result<Vec<MovieCandidate>> {
    let url = "https://www.tix.id";
    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await
        .with_context(|| format!("Failed to fetch TIX.ID catalog from {url}"))?;

    let html = resp
        .text()
        .await
        .context("Failed to read TIX.ID catalog response body")?;

    Ok(parse_catalog_html(&html))
}

/// Parses HTML content for `app.tix.id/movies/<slug>-<id>` links.
pub fn parse_catalog_html(html: &str) -> Vec<MovieCandidate> {
    let mut map: HashMap<String, MovieCandidate> = HashMap::new();

    for part in html.split("app.tix.id/movies/") {
        if let Some(end) = part.find('/') {
            let segment = &part[..end];
            if let Some(dash) = segment.rfind('-') {
                let slug = &segment[..dash];
                let id = &segment[dash + 1..];
                if id.chars().all(|c| c.is_ascii_digit()) && id.len() >= 15 {
                    if !map.contains_key(id) {
                        let display_name = slug
                            .split('-')
                            .map(|word| {
                                let mut chars = word.chars();
                                match chars.next() {
                                    None => String::new(),
                                    Some(first) => {
                                        first.to_uppercase().collect::<String>() + chars.as_str()
                                    }
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ");

                        map.insert(
                            id.to_string(),
                            MovieCandidate {
                                id: id.to_string(),
                                slug: slug.to_string(),
                                display_name,
                            },
                        );
                    }
                }
            }
        }
    }

    let mut result: Vec<MovieCandidate> = map.into_values().collect();
    result.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    result
}

/// Matches the target movie title against the list of candidates.
/// Returns the candidate with the highest similarity score if it meets or exceeds `threshold`.
pub fn find_best_match<'a>(
    candidates: &'a [MovieCandidate],
    target_title: &str,
    threshold: f64,
) -> Option<(&'a MovieCandidate, f64)> {
    let mut best: Option<(&'a MovieCandidate, f64)> = None;

    for cand in candidates {
        let score = fuzzy::calculate_similarity(target_title, &cand.display_name);
        if score >= threshold {
            if let Some((_, best_score)) = best {
                if score > best_score {
                    best = Some((cand, score));
                }
            } else {
                best = Some((cand, score));
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_catalog_html() {
        let sample = r#"
            <a href="https://app.tix.id/movies/avengers-endgame-encore-2094268965219024896/2026-10-09">Link</a>
            <a href="https://app.tix.id/movies/hasut-2093187333460410368/2026-10-09">Link</a>
            <a href="https://app.tix.id/movies/avengers-endgame-encore-2094268965219024896/2026-10-10">Duplicate</a>
        "#;

        let candidates = parse_catalog_html(sample);
        assert_eq!(candidates.len(), 2);

        let avengers = candidates
            .iter()
            .find(|c| c.id == "2094268965219024896")
            .unwrap();
        assert_eq!(avengers.slug, "avengers-endgame-encore");
        assert_eq!(avengers.display_name, "Avengers Endgame Encore");
    }

    #[test]
    fn test_find_best_match() {
        let candidates = vec![
            MovieCandidate {
                id: "1".into(),
                slug: "hasut".into(),
                display_name: "Hasut".into(),
            },
            MovieCandidate {
                id: "2".into(),
                slug: "avengers-endgame-encore".into(),
                display_name: "Avengers Endgame Encore".into(),
            },
            MovieCandidate {
                id: "3".into(),
                slug: "resident-evil".into(),
                display_name: "Resident Evil".into(),
            },
        ];

        let match_res = find_best_match(&candidates, "avenger endgame", DEFAULT_MATCH_THRESHOLD);
        assert!(match_res.is_some());
        let (cand, score) = match_res.unwrap();
        assert_eq!(cand.id, "2");
        assert!(score >= 0.8);

        let none_res = find_best_match(&candidates, "Interstellar 2", DEFAULT_MATCH_THRESHOLD);
        assert!(none_res.is_none());
    }

    #[test]
    fn test_multi_match_disambiguation_picks_closest() {
        let candidates = vec![
            MovieCandidate {
                id: "101".into(),
                slug: "avengers-endgame".into(),
                display_name: "Avengers Endgame".into(),
            },
            MovieCandidate {
                id: "102".into(),
                slug: "avengers-doomsday-imax".into(),
                display_name: "Avengers Doomsday IMAX".into(),
            },
            MovieCandidate {
                id: "103".into(),
                slug: "avengers-secret-wars".into(),
                display_name: "Avengers Secret Wars".into(),
            },
        ];

        // Searching for "avenger doomsday" should pick "Avengers Doomsday IMAX" over "Endgame" or "Secret Wars"
        let (best, score) = find_best_match(&candidates, "avenger doomsday", DEFAULT_MATCH_THRESHOLD).unwrap();
        assert_eq!(best.id, "102");
        assert_eq!(best.display_name, "Avengers Doomsday IMAX");
        assert!(score >= 0.85);
    }
}
