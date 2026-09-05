use std::{
    collections::BTreeMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use crate::{
    Action, CancellationToken, Provider, ProviderClass, ProviderDescriptor, ProviderError,
    SearchQuery, SearchResult,
    desktop_entry::{CatalogDiagnostics, DesktopApplication, DesktopCatalog},
    history::UsageSnapshot,
    ranking::{NormalizedText, ScoreScratch, usage_boost},
};

const GENERIC_NAME_PENALTY: i64 = 240;
const KEYWORD_PENALTY: i64 = 650;
const EXECUTABLE_PENALTY: i64 = 420;
const CATEGORY_PENALTY: i64 = 780;

pub struct ApplicationProvider {
    descriptor: ProviderDescriptor,
    applications: Arc<[DesktopApplication]>,
    prepared: Vec<PreparedApplication>,
    diagnostics: CatalogDiagnostics,
    usage: Arc<RwLock<UsageSnapshot>>,
    usage_enabled: AtomicBool,
    enabled: AtomicBool,
}

struct PreparedApplication {
    fields: Vec<(NormalizedText, i64)>,
    result_id: String,
    sort_name: String,
}

impl PreparedApplication {
    fn new(application: &DesktopApplication) -> Self {
        let fields = std::iter::once((application.name.as_str(), 0))
            .chain(
                application
                    .generic_name
                    .as_deref()
                    .map(|s| (s, GENERIC_NAME_PENALTY)),
            )
            .chain(
                application
                    .keywords
                    .iter()
                    .map(|s| (s.as_str(), KEYWORD_PENALTY)),
            )
            .chain(
                application
                    .executable_name
                    .as_deref()
                    .map(|s| (s, EXECUTABLE_PENALTY)),
            )
            .chain(
                application
                    .categories
                    .iter()
                    .map(|s| (s.as_str(), CATEGORY_PENALTY)),
            )
            .map(|(s, penalty)| (NormalizedText::new(s), penalty))
            .collect();
        Self {
            fields,
            result_id: format!("application:{}", application.desktop_id),
            sort_name: application.name.to_lowercase(),
        }
    }
}

impl ApplicationProvider {
    pub fn new(catalog: DesktopCatalog, usage: Arc<RwLock<UsageSnapshot>>) -> Self {
        let prepared = catalog
            .applications
            .iter()
            .map(PreparedApplication::new)
            .collect();
        Self {
            descriptor: ProviderDescriptor {
                id: "applications".into(),
                display_name: "Applications".into(),
                class: ProviderClass::Instant,
                default_priority: 100,
            },
            applications: catalog.applications.into(),
            prepared,
            diagnostics: catalog.diagnostics,
            usage,
            usage_enabled: AtomicBool::new(true),
            enabled: AtomicBool::new(true),
        }
    }

    pub fn application_count(&self) -> usize {
        self.applications.len()
    }

    pub fn diagnostics(&self) -> &CatalogDiagnostics {
        &self.diagnostics
    }

    pub fn replace_usage(&self, snapshot: UsageSnapshot) {
        match self.usage.write() {
            Ok(mut usage) => *usage = snapshot,
            Err(poisoned) => *poisoned.into_inner() = snapshot,
        }
    }

    pub fn set_usage_enabled(&self, enabled: bool) {
        self.usage_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn search_at(
        &self,
        query: &SearchQuery,
        cancellation: &CancellationToken,
        now: SystemTime,
    ) -> Result<Vec<SearchResult>, ProviderError> {
        let text = query.normalized_text();
        let prepared_query = NormalizedText::new(text);
        let mut scratch = ScoreScratch::default();
        let usage_enabled = self.usage_enabled.load(Ordering::Relaxed);
        let usage = match self.usage.read() {
            Ok(usage) => usage,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut candidates = Vec::with_capacity(self.applications.len().min(256));

        for (index, prepared) in self.prepared.iter().enumerate() {
            if index % 32 == 0 && cancellation.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }

            let stats = if usage_enabled {
                usage.get(&prepared.result_id)
            } else {
                Default::default()
            };
            let text_score = if text.is_empty() {
                Some(0)
            } else {
                prepared
                    .fields
                    .iter()
                    .filter_map(|(field, penalty)| {
                        scratch
                            .score(&prepared_query, field)
                            .map(|score| score - penalty)
                    })
                    .max()
            };

            if let Some(text_score) = text_score {
                candidates.push((
                    index,
                    text_score
                        + usage_boost(&stats, now)
                        + i64::from(self.descriptor.default_priority),
                ));
            }
        }
        drop(usage);

        let compare = |(left_index, left_score): &(usize, i64),
                       (right_index, right_score): &(usize, i64)| {
            right_score.cmp(left_score).then_with(|| {
                let left = &self.applications[*left_index];
                let right = &self.applications[*right_index];
                self.prepared[*left_index]
                    .sort_name
                    .cmp(&self.prepared[*right_index].sort_name)
                    .then_with(|| left.desktop_id.cmp(&right.desktop_id))
            })
        };
        if candidates.len() > query.max_results {
            candidates.select_nth_unstable_by(query.max_results, compare);
            candidates.truncate(query.max_results);
        }
        candidates.sort_unstable_by(compare);

        let mut results = Vec::with_capacity(candidates.len());
        for (index, score) in candidates {
            let application = &self.applications[index];
            let id = self.prepared[index].result_id.clone();
            let mut metadata = BTreeMap::new();
            metadata.insert("desktop_id".into(), application.desktop_id.clone());
            metadata.insert(
                "desktop_file".into(),
                application.source_path.to_string_lossy().into_owned(),
            );
            results.push(SearchResult {
                id,
                title: application.name.clone(),
                subtitle: application
                    .generic_name
                    .clone()
                    .or_else(|| Some("Application".into())),
                icon: application.icon.clone(),
                provider: self.descriptor.id.clone(),
                score,
                primary_action: Action::LaunchDesktopEntry {
                    desktop_id: application.desktop_id.clone(),
                },
                secondary_actions: application.secondary_actions.clone(),
                keywords: application.keywords.clone(),
                metadata,
            });
        }
        Ok(results)
    }
}

impl Provider for ApplicationProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn search(
        &self,
        query: &SearchQuery,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchResult>, ProviderError> {
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(vec![]);
        }
        self.search_at(query, cancellation, SystemTime::now())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crate::{GenerationClock, Icon, desktop_entry::DesktopCatalog, ranking::UsageStats};

    use super::*;

    fn app(id: &str, name: &str, keywords: &[&str]) -> DesktopApplication {
        DesktopApplication {
            desktop_id: format!("{id}.desktop"),
            source_path: PathBuf::from(format!("/test/{id}.desktop")),
            name: name.into(),
            generic_name: None,
            comment: None,
            keywords: keywords.iter().map(|value| (*value).into()).collect(),
            categories: vec![],
            executable_name: Some(id.into()),
            icon: Icon::default(),
            secondary_actions: vec![],
        }
    }

    fn provider(applications: Vec<DesktopApplication>) -> ApplicationProvider {
        ApplicationProvider::new(
            DesktopCatalog {
                applications,
                diagnostics: CatalogDiagnostics::default(),
            },
            Arc::new(RwLock::new(UsageSnapshot::default())),
        )
    }

    #[test]
    fn title_match_ranks_above_keyword_match() {
        let provider = provider(vec![
            app("terminal", "Terminal", &[]),
            app("helper", "Utility Helper", &["terminal"]),
        ]);
        let token = GenerationClock::new().next();
        let query = SearchQuery::new(token.generation(), "terminal", 10);
        let results = provider
            .search_at(&query, &token, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(results[0].title, "Terminal");
    }

    #[test]
    fn usage_improves_close_matches_without_overriding_text() {
        let provider = provider(vec![
            app("code", "Code", &[]),
            app("codium", "Codium", &[]),
            app("calendar", "Calendar", &["code"]),
        ]);
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000);
        let mut values = UsageSnapshot::default();
        values.insert(
            "application:codium.desktop",
            UsageStats {
                launch_count: 30,
                last_used_unix_seconds: Some(1_999_900),
            },
        );
        values.insert(
            "application:calendar.desktop",
            UsageStats {
                launch_count: 100_000,
                last_used_unix_seconds: Some(1_999_900),
            },
        );
        provider.replace_usage(values);

        let token = GenerationClock::new().next();
        let query = SearchQuery::new(token.generation(), "cod", 10);
        let results = provider.search_at(&query, &token, now).unwrap();
        assert_eq!(results[0].title, "Codium");
        assert_ne!(results[0].title, "Calendar");
        provider.set_usage_enabled(false);
        let results = provider.search_at(&query, &token, now).unwrap();
        assert_eq!(results[0].title, "Code");
        provider.set_usage_enabled(true);
        assert_eq!(
            provider.search_at(&query, &token, now).unwrap()[0].title,
            "Codium"
        );
    }

    #[test]
    fn cancelled_search_returns_no_stale_batch() {
        let provider = provider(
            (0..100)
                .map(|n| app(&format!("app{n}"), "App", &[]))
                .collect(),
        );
        let clock = GenerationClock::new();
        let old = clock.next();
        let _new = clock.next();
        let query = SearchQuery::new(old.generation(), "app", 10);
        assert!(matches!(
            provider.search(&query, &old),
            Err(ProviderError::Cancelled)
        ));
    }
}
