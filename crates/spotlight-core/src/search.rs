use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::{
    GenerationClock, Provider, ProviderError, ProviderId, QueryGeneration, SearchQuery,
    SearchResult,
    performance::{PerformanceRecorder, SearchTiming},
};

#[derive(Clone, Debug)]
pub enum SearchUpdate {
    Started {
        generation: QueryGeneration,
    },
    ProviderBatch {
        generation: QueryGeneration,
        provider: ProviderId,
        results: Vec<SearchResult>,
        elapsed: Duration,
    },
    ProviderFailed {
        generation: QueryGeneration,
        provider: ProviderId,
        message: String,
    },
    Finished {
        generation: QueryGeneration,
        elapsed: Duration,
    },
}

impl SearchUpdate {
    pub fn generation(&self) -> QueryGeneration {
        match self {
            Self::Started { generation }
            | Self::ProviderBatch { generation, .. }
            | Self::ProviderFailed { generation, .. }
            | Self::Finished { generation, .. } => *generation,
        }
    }
}

/// Single event-driven coordinator that always processes the most recent query.
///
/// Replacing a pending slot is intentional: intermediate keystrokes are not useful.
/// Running providers receive a generation token and are expected to stop promptly.
pub struct SearchEngine {
    clock: GenerationClock,
    slot: Arc<LatestQuerySlot>,
    worker: Option<thread::JoinHandle<()>>,
    performance: Arc<PerformanceRecorder>,
}

impl SearchEngine {
    pub fn start(
        mut providers: Vec<Arc<dyn Provider>>,
    ) -> (Self, async_channel::Receiver<SearchUpdate>) {
        providers.sort_by(|left, right| {
            left.descriptor()
                .class
                .cmp(&right.descriptor().class)
                .then_with(|| {
                    right
                        .descriptor()
                        .default_priority
                        .cmp(&left.descriptor().default_priority)
                })
                .then_with(|| left.descriptor().id.cmp(&right.descriptor().id))
        });

        let clock = GenerationClock::new();
        let slot = Arc::new(LatestQuerySlot::default());
        let performance = Arc::new(PerformanceRecorder::default());
        let (updates_tx, updates_rx) = async_channel::unbounded();
        let worker_slot = Arc::clone(&slot);
        let worker_clock = clock.clone();
        let worker_performance = Arc::clone(&performance);

        let worker = thread::Builder::new()
            .name("spotlight-search".into())
            .spawn(move || {
                run_worker(
                    providers,
                    worker_clock,
                    worker_slot,
                    updates_tx,
                    worker_performance,
                );
            })
            .expect("failed to create the search worker thread");

        (
            Self {
                clock,
                slot,
                worker: Some(worker),
                performance,
            },
            updates_rx,
        )
    }

    pub fn submit(&self, text: impl Into<String>, max_results: usize) -> QueryGeneration {
        let token = self.clock.next();
        let generation = token.generation();
        self.slot.submit(PendingQuery {
            query: SearchQuery::new(generation, text, max_results),
            token,
        });
        generation
    }

    pub fn performance(&self) -> Arc<PerformanceRecorder> {
        Arc::clone(&self.performance)
    }

    /// Invalidate pending/in-flight results without scheduling another search.
    pub fn cancel(&self) -> QueryGeneration {
        self.clock.next().generation()
    }
}

impl Drop for SearchEngine {
    fn drop(&mut self) {
        self.clock.cancel_current();
        self.slot.close();
        if let Some(worker) = self.worker.take()
            && worker.thread().id() != thread::current().id()
        {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct LatestQuerySlot {
    state: Mutex<SlotState>,
    ready: Condvar,
}

#[derive(Default)]
struct SlotState {
    pending: Option<PendingQuery>,
    closed: bool,
}

struct PendingQuery {
    query: SearchQuery,
    token: crate::CancellationToken,
}

impl LatestQuerySlot {
    fn submit(&self, query: PendingQuery) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending = Some(query);
        self.ready.notify_one();
    }

    fn take(&self) -> Option<PendingQuery> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.pending.is_none() && !state.closed {
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if state.closed {
            None
        } else {
            state.pending.take()
        }
    }

    fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        state.pending = None;
        self.ready.notify_all();
    }
}

fn run_worker(
    providers: Vec<Arc<dyn Provider>>,
    clock: GenerationClock,
    slot: Arc<LatestQuerySlot>,
    updates: async_channel::Sender<SearchUpdate>,
    performance: Arc<PerformanceRecorder>,
) {
    while let Some(pending) = slot.take() {
        let generation = pending.query.generation;
        let search_started = Instant::now();
        if updates
            .send_blocking(SearchUpdate::Started { generation })
            .is_err()
        {
            break;
        }

        for provider in &providers {
            if pending.token.is_cancelled() {
                break;
            }
            let descriptor = provider.descriptor().clone();
            let provider_started = Instant::now();
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                provider.search(&pending.query, &pending.token)
            }));
            let elapsed = provider_started.elapsed();

            if pending.token.is_cancelled() {
                continue;
            }

            match outcome {
                Ok(Ok(mut results)) => {
                    results.sort_by(SearchResult::stable_cmp);
                    results.truncate(pending.query.max_results);
                    performance.record(SearchTiming {
                        generation,
                        provider: descriptor.id.clone(),
                        elapsed,
                        result_count: results.len(),
                        succeeded: true,
                    });
                    if updates
                        .send_blocking(SearchUpdate::ProviderBatch {
                            generation,
                            provider: descriptor.id,
                            results,
                            elapsed,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(Err(ProviderError::Cancelled)) => {}
                Ok(Err(error)) => {
                    performance.record(SearchTiming {
                        generation,
                        provider: descriptor.id.clone(),
                        elapsed,
                        result_count: 0,
                        succeeded: false,
                    });
                    if updates
                        .send_blocking(SearchUpdate::ProviderFailed {
                            generation,
                            provider: descriptor.id,
                            message: error.to_string(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => {
                    performance.record(SearchTiming {
                        generation,
                        provider: descriptor.id.clone(),
                        elapsed,
                        result_count: 0,
                        succeeded: false,
                    });
                    if updates
                        .send_blocking(SearchUpdate::ProviderFailed {
                            generation,
                            provider: descriptor.id,
                            message: "provider panicked; it has been isolated from this search"
                                .into(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }

        if !pending.token.is_cancelled()
            && clock.current() == generation
            && updates
                .send_blocking(SearchUpdate::Finished {
                    generation,
                    elapsed: search_started.elapsed(),
                })
                .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicBool, Ordering},
    };

    use crate::{Action, Icon, ProviderDescriptor};

    use super::*;

    struct TestProvider {
        descriptor: ProviderDescriptor,
        fail: bool,
        slow: bool,
        searched_old: Arc<AtomicBool>,
        started: Option<std::sync::mpsc::SyncSender<()>>,
    }

    impl TestProvider {
        fn new(id: &str) -> Self {
            Self {
                descriptor: ProviderDescriptor {
                    id: id.into(),
                    display_name: id.into(),
                    class: crate::ProviderClass::Instant,
                    default_priority: 0,
                },
                fail: false,
                slow: false,
                searched_old: Arc::new(AtomicBool::new(false)),
                started: None,
            }
        }
    }

    impl Provider for TestProvider {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        fn search(
            &self,
            query: &SearchQuery,
            cancellation: &crate::CancellationToken,
        ) -> Result<Vec<SearchResult>, ProviderError> {
            if query.text == "old"
                && let Some(started) = &self.started
            {
                started.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !cancellation.is_cancelled() {
                    assert!(
                        Instant::now() < deadline,
                        "running search was not cancelled"
                    );
                    thread::sleep(Duration::from_micros(100));
                }
                self.searched_old.store(true, Ordering::Release);
                return Err(ProviderError::Cancelled);
            }
            if self.fail {
                return Err(ProviderError::Message("expected failure".into()));
            }
            if self.slow {
                for _ in 0..200 {
                    if cancellation.is_cancelled() {
                        self.searched_old.store(true, Ordering::Release);
                        return Err(ProviderError::Cancelled);
                    }
                    thread::sleep(Duration::from_micros(100));
                }
            }
            Ok(vec![SearchResult {
                id: format!("{}:{}", self.descriptor.id.0, query.text),
                title: query.text.clone(),
                subtitle: None,
                icon: Icon::default(),
                provider: self.descriptor.id.clone(),
                score: 1,
                primary_action: Action::OpenSettings,
                secondary_actions: vec![],
                keywords: vec![],
                metadata: BTreeMap::new(),
            }])
        }
    }

    #[test]
    fn emits_a_complete_generation() {
        let (engine, updates) = SearchEngine::start(vec![Arc::new(TestProvider::new("test"))]);
        let generation = engine.submit("hello", 8);
        let received = (0..3)
            .map(|_| updates.recv_blocking().unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(received[0], SearchUpdate::Started { .. }));
        assert!(matches!(received[1], SearchUpdate::ProviderBatch { .. }));
        assert!(matches!(received[2], SearchUpdate::Finished { .. }));
        assert!(
            received
                .iter()
                .all(|update| update.generation() == generation)
        );
    }

    #[test]
    fn a_failed_provider_does_not_block_the_next_provider() {
        let mut failed = TestProvider::new("failed");
        failed.fail = true;
        let good = TestProvider::new("good");
        let (engine, updates) = SearchEngine::start(vec![Arc::new(failed), Arc::new(good)]);
        let generation = engine.submit("query", 8);
        let mut saw_failure = false;
        let mut saw_good = false;
        while let Ok(update) = updates.recv_blocking() {
            if update.generation() != generation {
                continue;
            }
            match update {
                SearchUpdate::ProviderFailed { provider, .. } if provider.0 == "failed" => {
                    saw_failure = true;
                }
                SearchUpdate::ProviderBatch { provider, .. } if provider.0 == "good" => {
                    saw_good = true;
                }
                SearchUpdate::Finished { .. } => break,
                _ => {}
            }
        }
        assert!(saw_failure && saw_good);
    }

    #[test]
    fn newer_query_cancels_and_suppresses_old_results() {
        let mut provider = TestProvider::new("slow");
        provider.slow = true;
        let (started, entered) = std::sync::mpsc::sync_channel(1);
        provider.started = Some(started);
        let cancellation_observed = Arc::clone(&provider.searched_old);
        let (engine, updates) = SearchEngine::start(vec![Arc::new(provider)]);
        let old = engine.submit("old", 8);
        // Started is a pipeline event, not proof that a worker entered search.
        // Synchronize with the provider so this assertion tests cancellation
        // of running work, not the equally valid cancellation of queued work.
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let fresh = engine.submit("fresh", 8);

        let mut saw_old_batch = false;
        let mut saw_fresh_batch = false;
        while let Ok(update) = updates.recv_blocking() {
            match update {
                SearchUpdate::ProviderBatch { generation, .. } if generation == old => {
                    saw_old_batch = true;
                }
                SearchUpdate::ProviderBatch { generation, .. } if generation == fresh => {
                    saw_fresh_batch = true;
                }
                SearchUpdate::Finished { generation, .. } if generation == fresh => break,
                _ => {}
            }
        }
        assert!(!saw_old_batch);
        assert!(saw_fresh_batch);
        assert!(cancellation_observed.load(Ordering::Acquire));
    }
}
