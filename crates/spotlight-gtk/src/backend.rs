use std::{
    cell::Cell,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
};

use async_channel::{Receiver, Sender};
use spotlight_core::{
    Provider, SearchEngine,
    desktop_entry::{CatalogDiagnostics, DesktopEnvironment, load_desktop_catalog},
    history::{UsageSnapshot, UsageStore},
    providers::applications::ApplicationProvider,
    providers::calculator::CalculatorProvider,
    search::SearchUpdate,
};

pub struct Backend {
    engine: SearchEngine,
    updates: Option<Receiver<SearchUpdate>>,
    applications: Arc<ApplicationProvider>,
    calculator: Arc<CalculatorProvider>,
    history: HistoryService,
    history_enabled: Cell<bool>,
    warning: Option<String>,
}

impl Backend {
    pub fn initialize(history_path: PathBuf, usage_history_enabled: bool) -> Receiver<Backend> {
        let (sender, receiver) = async_channel::bounded(1);
        thread::Builder::new()
            .name("spotlight-initialize".into())
            .spawn(move || {
                let catalog = load_desktop_catalog(&DesktopEnvironment::from_process());
                let (store, snapshot, warning) = if usage_history_enabled {
                    match UsageStore::open(&history_path) {
                        Ok(store) => match store.snapshot() {
                            Ok(snapshot) => (Some(store), snapshot, None),
                            Err(error) => (
                                Some(store),
                                UsageSnapshot::default(),
                                Some(error.to_string()),
                            ),
                        },
                        Err(error) => (None, UsageSnapshot::default(), Some(error.to_string())),
                    }
                } else {
                    (
                        None,
                        UsageSnapshot::default(),
                        UsageStore::protect_existing(&history_path)
                            .err()
                            .map(|error| error.to_string()),
                    )
                };

                let applications = Arc::new(ApplicationProvider::new(
                    catalog,
                    Arc::new(RwLock::new(snapshot)),
                ));
                let provider: Arc<dyn Provider> = applications.clone();
                applications.set_usage_enabled(usage_history_enabled);
                let calculator = Arc::new(CalculatorProvider::default());
                let (engine, updates) = SearchEngine::start(vec![provider, calculator.clone()]);
                let history = HistoryService::start(
                    store,
                    history_path,
                    applications.clone(),
                    usage_history_enabled,
                );
                let backend = Backend {
                    engine,
                    updates: Some(updates),
                    applications,
                    calculator,
                    history,
                    history_enabled: Cell::new(usage_history_enabled),
                    warning,
                };
                let _ = sender.send_blocking(backend);
            })
            .expect("failed to create initialization worker");
        receiver
    }

    pub fn engine(&self) -> &SearchEngine {
        &self.engine
    }

    pub fn configure_search(&self, settings: &spotlight_core::settings::SearchSettings) {
        self.applications.set_enabled(settings.applications_enabled);
        self.calculator.set_enabled(settings.calculator_enabled);
    }

    pub fn take_updates(&mut self) -> Receiver<SearchUpdate> {
        self.updates
            .take()
            .expect("search update receiver can only be taken once")
    }

    pub fn record_launch(&self, result_id: &str) {
        if self.history_enabled.get() {
            self.history.record(result_id);
        }
    }

    pub fn clear_history(&self) -> Receiver<Result<(), String>> {
        self.history.clear()
    }

    pub fn set_history_enabled(&self, enabled: bool) {
        if self.history_enabled.replace(enabled) != enabled {
            self.applications.set_usage_enabled(enabled);
            let _ = self
                .history
                .commands
                .try_send(HistoryCommand::Enabled(enabled));
        }
    }

    pub fn application_count(&self) -> usize {
        self.applications.application_count()
    }

    pub fn catalog_diagnostics(&self) -> &CatalogDiagnostics {
        self.applications.diagnostics()
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

enum HistoryCommand {
    Record(String),
    Clear(Sender<Result<(), String>>),
    Enabled(bool),
}

struct HistoryService {
    commands: Sender<HistoryCommand>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HistoryService {
    fn start(
        mut store: Option<UsageStore>,
        path: PathBuf,
        applications: Arc<ApplicationProvider>,
        mut enabled: bool,
    ) -> Self {
        // Only explicit launches/settings enqueue here, never keystrokes. Do
        // not drop privacy/control changes behind a bounded queue of launches.
        let (commands, receiver) = async_channel::unbounded();
        let worker = thread::Builder::new()
            .name("spotlight-history".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv_blocking() {
                    if let HistoryCommand::Enabled(value) = command {
                        enabled = value;
                        if !enabled {
                            store.take();
                            continue;
                        }
                    }
                    if matches!(command, HistoryCommand::Record(_)) && !enabled {
                        continue;
                    }
                    let reply = match &command {
                        HistoryCommand::Clear(reply) => Some(reply.clone()),
                        _ => None,
                    };
                    let operation = (|| {
                        if store.is_none() {
                            store = Some(UsageStore::open(&path)?);
                        }
                        let store = store.as_mut().expect("store opened above");
                        match command {
                            HistoryCommand::Record(result_id) => {
                                store.record_launch(&result_id)?;
                            }
                            HistoryCommand::Clear(_) => store.clear()?,
                            HistoryCommand::Enabled(_) => (),
                        }
                        store.snapshot()
                    })();
                    match operation {
                        Ok(snapshot) => {
                            applications.replace_usage(snapshot);
                            if let Some(reply) = reply {
                                let _ = reply.try_send(Ok(()));
                            }
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "usage history operation failed");
                            if let Some(reply) = reply {
                                let _ = reply.try_send(Err(error.to_string()));
                            }
                        }
                    }
                    if !enabled {
                        store.take();
                    }
                }
            })
            .expect("failed to create history worker");
        Self {
            commands,
            worker: Some(worker),
        }
    }

    fn record(&self, result_id: &str) {
        let _ = self
            .commands
            .try_send(HistoryCommand::Record(result_id.to_owned()));
    }

    fn clear(&self) -> Receiver<Result<(), String>> {
        let (sender, receiver) = async_channel::bounded(1);
        if self
            .commands
            .try_send(HistoryCommand::Clear(sender.clone()))
            .is_err()
        {
            let _ = sender.try_send(Err(
                "History worker is unavailable; history was not cleared".into(),
            ));
        }
        receiver
    }
}

impl Drop for HistoryService {
    fn drop(&mut self) {
        self.commands.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotlight_core::desktop_entry::DesktopCatalog;

    #[test]
    fn disabled_learning_still_protects_legacy_history() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("spotlight-linux");
        let path = parent.join("history.sqlite3");
        let mut store = UsageStore::open(&path).unwrap();
        store.record_launch_at("legacy", 100).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        for suffix in ["", "-wal", "-shm"] {
            std::fs::set_permissions(
                format!("{}{}", path.display(), suffix),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        let backend = Backend::initialize(path.clone(), false)
            .recv_blocking()
            .unwrap();
        assert!(backend.warning().is_none());
        assert_eq!(
            std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for suffix in ["", "-wal", "-shm"] {
            assert_eq!(
                std::fs::metadata(format!("{}{}", path.display(), suffix))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert_eq!(store.stats("legacy").unwrap().unwrap().launch_count, 1);
    }

    #[test]
    fn clear_reports_storage_failure_and_stopped_worker() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("invalid.sqlite3");
        std::fs::write(&path, b"not a SQLite database").unwrap();
        let provider = Arc::new(ApplicationProvider::new(
            DesktopCatalog {
                applications: vec![],
                diagnostics: CatalogDiagnostics::default(),
            },
            Arc::new(RwLock::new(UsageSnapshot::default())),
        ));
        let service = HistoryService::start(None, path, provider, false);
        assert!(service.clear().recv_blocking().unwrap().is_err());
        service.commands.close();
        assert!(service.clear().recv_blocking().unwrap().is_err());
    }

    #[test]
    fn history_can_be_enabled_disabled_and_cleared_without_restart() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history.sqlite");
        let provider = Arc::new(ApplicationProvider::new(
            DesktopCatalog {
                applications: vec![],
                diagnostics: CatalogDiagnostics::default(),
            },
            Arc::new(RwLock::new(UsageSnapshot::default())),
        ));
        let service = HistoryService::start(None, path.clone(), provider.clone(), false);
        service.record("ignored-before");
        service
            .commands
            .try_send(HistoryCommand::Enabled(true))
            .unwrap();
        service.record("kept");
        service
            .commands
            .try_send(HistoryCommand::Enabled(false))
            .unwrap();
        service.record("ignored-after");
        drop(service); // Flush pending commands before checking persisted data.
        let snapshot = UsageStore::open(&path).unwrap().snapshot().unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot.get("kept").launch_count, 1);
        let service = HistoryService::start(None, path.clone(), provider, false);
        service.clear().recv_blocking().unwrap().unwrap();
        drop(service);
        assert!(
            UsageStore::open(&path)
                .unwrap()
                .snapshot()
                .unwrap()
                .is_empty()
        );
    }
}
