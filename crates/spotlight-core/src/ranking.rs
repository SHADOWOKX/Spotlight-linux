use std::time::{Duration, SystemTime, UNIX_EPOCH};

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const NO_MATCH: i64 = i64::MIN / 4;
const CHARACTER_SCORE: i64 = 100;
const BOUNDARY_BONUS: i64 = 115;
const START_BONUS: i64 = 260;
const CONTIGUOUS_BONUS: i64 = 145;
const GAP_PENALTY: i64 = 9;
const START_PENALTY: i64 = 3;
const EXACT_BONUS: i64 = 5_000;
const PREFIX_BONUS: i64 = 2_200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuzzyMatch {
    pub score: i64,
    /// Character offsets in the normalized candidate, suitable for later highlighting.
    pub positions: Vec<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UsageStats {
    pub launch_count: u64,
    pub last_used_unix_seconds: Option<i64>,
}

/// Unicode-aware subsequence matcher with O(query × candidate) complexity.
///
/// Diacritics and case do not affect matching. Exact prefixes, word boundaries,
/// and contiguous runs receive large bonuses. The dynamic program avoids the
/// surprising choices made by a purely greedy matcher while remaining cheap for
/// an in-memory application catalog.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    let query = NormalizedText::new(query);
    let candidate = NormalizedText::new(candidate);

    if query.chars.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: vec![],
        });
    }
    if query.chars.len() > candidate.chars.len() {
        return None;
    }

    let width = candidate.chars.len();
    let height = query.chars.len();
    let mut scores = vec![vec![NO_MATCH; width]; height];
    let mut previous = vec![vec![usize::MAX; width]; height];

    for (column, candidate_char) in candidate.chars.iter().enumerate() {
        if *candidate_char == query.chars[0] {
            scores[0][column] = character_value(&candidate, column)
                + if column == 0 { START_BONUS } else { 0 }
                - (column as i64 * START_PENALTY);
        }
    }

    for row in 1..height {
        let mut best_gap_value = NO_MATCH;
        let mut best_gap_column = usize::MAX;

        for column in 0..width {
            if column >= 2 {
                let gap_source = column - 2;
                if scores[row - 1][gap_source] != NO_MATCH {
                    let value = scores[row - 1][gap_source] + GAP_PENALTY * gap_source as i64;
                    if value > best_gap_value {
                        best_gap_value = value;
                        best_gap_column = gap_source;
                    }
                }
            }

            if candidate.chars[column] != query.chars[row] {
                continue;
            }

            let contiguous = if column > 0 && scores[row - 1][column - 1] != NO_MATCH {
                scores[row - 1][column - 1] + CONTIGUOUS_BONUS
            } else {
                NO_MATCH
            };

            let gapped = if best_gap_value != NO_MATCH {
                best_gap_value - GAP_PENALTY * (column as i64 - 1)
            } else {
                NO_MATCH
            };

            let (transition, predecessor) = if contiguous >= gapped {
                (contiguous, column.saturating_sub(1))
            } else {
                (gapped, best_gap_column)
            };

            if transition != NO_MATCH {
                scores[row][column] = transition + character_value(&candidate, column);
                previous[row][column] = predecessor;
            }
        }
    }

    let (mut column, mut score) = scores[height - 1]
        .iter()
        .copied()
        .enumerate()
        .max_by_key(|&(column, value)| (value, std::cmp::Reverse(column)))?;

    if score == NO_MATCH {
        return None;
    }

    let mut positions = vec![0; height];
    for row in (0..height).rev() {
        positions[row] = column;
        if row > 0 {
            column = previous[row][column];
        }
    }

    if query.chars == candidate.chars {
        score += EXACT_BONUS;
    } else if candidate.chars.starts_with(&query.chars) {
        score += PREFIX_BONUS;
    }

    Some(FuzzyMatch { score, positions })
}

/// Returns the best matching field with a field-specific penalty.
pub fn best_field_score<'a>(
    query: &str,
    fields: impl IntoIterator<Item = (&'a str, i64)>,
) -> Option<i64> {
    fields
        .into_iter()
        .filter_map(|(field, penalty)| fuzzy_match(query, field).map(|m| m.score - penalty))
        .max()
}

/// A deliberately bounded usage boost: textual relevance always remains dominant.
pub fn usage_boost(stats: &UsageStats, now: SystemTime) -> i64 {
    let frequency = if stats.launch_count == 0 {
        0
    } else {
        (((stats.launch_count + 1) as f64).ln() * 150.0).round() as i64
    }
    .min(850);

    let recency = stats
        .last_used_unix_seconds
        .and_then(|timestamp| {
            let used = UNIX_EPOCH.checked_add(Duration::from_secs(timestamp.max(0) as u64))?;
            now.duration_since(used).ok()
        })
        .map(|age| match age.as_secs() {
            0..=3_600 => 300,
            3_601..=86_400 => 240,
            86_401..=604_800 => 170,
            604_801..=2_592_000 => 90,
            2_592_001..=7_776_000 => 35,
            _ => 0,
        })
        .unwrap_or(0);

    // Prevent a heavily used item with a weak text match from becoming surprising.
    (frequency + recency).min(1_200)
}

fn character_value(candidate: &NormalizedText, column: usize) -> i64 {
    CHARACTER_SCORE
        + if candidate.boundaries[column] {
            BOUNDARY_BONUS
        } else {
            0
        }
}

#[derive(Debug)]
pub(crate) struct NormalizedText {
    chars: Vec<char>,
    boundaries: Vec<bool>,
}

impl NormalizedText {
    pub(crate) fn new(value: &str) -> Self {
        let mut chars = Vec::with_capacity(value.len());
        let mut boundaries = Vec::with_capacity(value.len());
        let mut previous_original: Option<char> = None;

        for original in value.trim().chars() {
            let is_boundary = previous_original.is_none_or(|previous| {
                !previous.is_alphanumeric() || (previous.is_lowercase() && original.is_uppercase())
            });
            let mut first_output = true;

            for lowered in original.to_lowercase() {
                for normalized in lowered.to_string().nfd() {
                    if is_combining_mark(normalized) {
                        continue;
                    }
                    chars.push(normalized);
                    boundaries.push(is_boundary && first_output);
                    first_output = false;
                }
            }
            previous_original = Some(original);
        }

        Self { chars, boundaries }
    }
}

/// Score-only DP for indexed fields. Two rows are reused across the entire
/// catalog; highlight paths are only needed by callers of `fuzzy_match`.
#[derive(Default)]
pub(crate) struct ScoreScratch {
    previous: Vec<i64>,
    current: Vec<i64>,
}

impl ScoreScratch {
    pub(crate) fn score(
        &mut self,
        query: &NormalizedText,
        candidate: &NormalizedText,
    ) -> Option<i64> {
        if query.chars.is_empty() {
            return Some(0);
        }
        // Reject non-subsequences before allocating or running the DP.
        let mut matched = 0;
        for character in &candidate.chars {
            if *character == query.chars[matched] {
                matched += 1;
                if matched == query.chars.len() {
                    break;
                }
            }
        }
        if matched != query.chars.len() {
            return None;
        }
        let width = candidate.chars.len();
        self.previous.resize(width, NO_MATCH);
        self.previous.fill(NO_MATCH);
        self.current.resize(width, NO_MATCH);
        for (column, character) in candidate.chars.iter().enumerate() {
            if *character == query.chars[0] {
                self.previous[column] = character_value(candidate, column)
                    + if column == 0 { START_BONUS } else { 0 }
                    - column as i64 * START_PENALTY;
            }
        }
        for character in query.chars.iter().skip(1) {
            self.current.fill(NO_MATCH);
            let mut best_gap = NO_MATCH;
            for column in 0..width {
                if column >= 2 && self.previous[column - 2] != NO_MATCH {
                    best_gap =
                        best_gap.max(self.previous[column - 2] + GAP_PENALTY * (column as i64 - 2));
                }
                if candidate.chars[column] != *character {
                    continue;
                }
                let contiguous = if column > 0 && self.previous[column - 1] != NO_MATCH {
                    self.previous[column - 1] + CONTIGUOUS_BONUS
                } else {
                    NO_MATCH
                };
                let gapped = if best_gap != NO_MATCH {
                    best_gap - GAP_PENALTY * (column as i64 - 1)
                } else {
                    NO_MATCH
                };
                let transition = contiguous.max(gapped);
                if transition != NO_MATCH {
                    self.current[column] = transition + character_value(candidate, column);
                }
            }
            std::mem::swap(&mut self.previous, &mut self.current);
        }
        let mut score = *self.previous.iter().max()?;
        if score == NO_MATCH {
            return None;
        }
        if query.chars == candidate.chars {
            score += EXACT_BONUS;
        } else if candidate.chars.starts_with(&query.chars) {
            score += PREFIX_BONUS;
        }
        Some(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_scores_equal_highlighting_matcher() {
        let mut scratch = ScoreScratch::default();
        // Exhaust short strings to exercise contiguous/gapped ties and reuse
        // across different row widths, then cover Unicode normalization.
        let mut words = vec![String::new()];
        for _ in 0..4 {
            let previous = words.clone();
            for word in previous {
                for ch in ['a', 'B', ' '] {
                    words.push(format!("{word}{ch}"));
                }
            }
        }
        words.extend(
            [
                "Résumé Viewer",
                "VisualStudioCode",
                "محرر",
                "Straße",
                "e\u{301}",
            ]
            .map(str::to_owned),
        );
        for query in &words {
            let prepared_query = NormalizedText::new(query);
            for candidate in &words {
                assert_eq!(
                    scratch.score(&prepared_query, &NormalizedText::new(candidate)),
                    fuzzy_match(query, candidate).map(|m| m.score),
                    "{query:?} / {candidate:?}"
                );
            }
        }
    }

    #[test]
    fn exact_and_prefix_matches_beat_substrings() {
        let exact = fuzzy_match("terminal", "Terminal").unwrap().score;
        let prefix = fuzzy_match("term", "Terminal").unwrap().score;
        let substring = fuzzy_match("term", "XTerm Helper").unwrap().score;
        assert!(exact > prefix);
        assert!(prefix > substring);
    }

    #[test]
    fn word_and_camel_boundaries_are_rewarded() {
        let boundary = fuzzy_match("vsc", "Visual Studio Code").unwrap().score;
        let camel = fuzzy_match("vsc", "VisualStudioCode").unwrap().score;
        let scattered = fuzzy_match("vsc", "Obvious calculation").unwrap().score;
        assert!(boundary > scattered);
        assert!(camel > scattered);
    }

    #[test]
    fn matching_is_case_and_diacritic_insensitive() {
        assert!(fuzzy_match("resume", "Résumé Viewer").is_some());
        assert_eq!(
            fuzzy_match("résumé", "Resume").unwrap().score,
            fuzzy_match("resume", "Resume").unwrap().score
        );
    }

    #[test]
    fn non_subsequence_does_not_match() {
        assert!(fuzzy_match("firefox", "Files").is_none());
    }

    #[test]
    fn recent_usage_is_bounded_and_decays() {
        let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
        let recent = UsageStats {
            launch_count: 10_000,
            last_used_unix_seconds: Some(9_999_000),
        };
        let old = UsageStats {
            launch_count: 10_000,
            last_used_unix_seconds: Some(1),
        };
        assert!(usage_boost(&recent, now) > usage_boost(&old, now));
        assert!(usage_boost(&recent, now) <= 1_200);
    }
}
