use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cancellation::QueryGeneration;

/// Stable, serialization-friendly provider identifier.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Icon description that the frontend resolves without making core depend on GTK.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Icon {
    Themed(String),
    File(String),
    Text(String),
}

impl Default for Icon {
    fn default() -> Self {
        Self::Themed("application-x-executable-symbolic".into())
    }
}

/// Typed actions are interpreted by a trusted platform adapter.
///
/// There is intentionally no general `Shell(String)` variant. A future deliberate
/// shell mode must use a separate, explicitly enabled action with argv boundaries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    LaunchDesktopEntry {
        desktop_id: String,
    },
    LaunchDesktopAction {
        desktop_id: String,
        action_id: String,
    },
    OpenSettings,
    CopyText {
        text: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SecondaryAction {
    pub title: String,
    pub icon: Icon,
    pub shortcut_hint: Option<String>,
    pub action: Action,
}

/// A provider result before or after unified ranking.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Icon,
    pub provider: ProviderId,
    pub score: i64,
    pub primary_action: Action,
    pub secondary_actions: Vec<SecondaryAction>,
    pub keywords: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

impl SearchResult {
    pub fn stable_cmp(left: &Self, right: &Self) -> std::cmp::Ordering {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchQuery {
    pub generation: QueryGeneration,
    pub text: String,
    pub max_results: usize,
}

impl SearchQuery {
    pub fn new(generation: QueryGeneration, text: impl Into<String>, max_results: usize) -> Self {
        Self {
            generation,
            text: text.into(),
            max_results: max_results.clamp(1, 100),
        }
    }

    pub fn normalized_text(&self) -> &str {
        self.text.trim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(id: &str, title: &str, score: i64) -> SearchResult {
        SearchResult {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            icon: Icon::default(),
            provider: "test".into(),
            score,
            primary_action: Action::OpenSettings,
            secondary_actions: vec![],
            keywords: vec![],
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn stable_order_uses_title_and_id_for_ties() {
        let mut values = [
            result("b", "Zulu", 10),
            result("c", "Alpha", 10),
            result("a", "Alpha", 10),
            result("high", "Last", 20),
        ];
        values.sort_by(SearchResult::stable_cmp);
        assert_eq!(
            values.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["high", "a", "c", "b"]
        );
    }
}
