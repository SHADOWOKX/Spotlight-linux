use std::{
    collections::HashMap,
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use thiserror::Error;

use crate::ranking::UsageStats;

#[derive(Clone, Debug, Default)]
pub struct UsageSnapshot {
    values: HashMap<String, UsageStats>,
}

impl UsageSnapshot {
    pub fn get(&self, result_id: &str) -> UsageStats {
        self.values.get(result_id).cloned().unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn insert(&mut self, result_id: impl Into<String>, stats: UsageStats) {
        self.values.insert(result_id.into(), stats);
    }
}

impl FromIterator<(String, UsageStats)> for UsageSnapshot {
    fn from_iter<T: IntoIterator<Item = (String, UsageStats)>>(iter: T) -> Self {
        Self {
            values: iter.into_iter().collect(),
        }
    }
}

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("could not create history directory: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("could not secure launch history: {0}")]
    Privacy(#[source] std::io::Error),
    #[error("history database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// SQLite-backed launch history. Search never holds or queries this connection;
/// callers refresh an in-memory `UsageSnapshot` after a successful action.
pub struct UsageStore {
    connection: Connection,
    path: PathBuf,
}

impl UsageStore {
    /// Protect legacy data even when learning is disabled, without creating a database.
    pub fn protect_existing(path: impl AsRef<Path>) -> Result<(), HistoryError> {
        secure_history(path.as_ref(), false).map_err(HistoryError::Privacy)
    }

    /// The parent is an application-private directory, not a shared XDG root.
    /// Bare filenames remain supported without changing the working directory's mode.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, HistoryError> {
        let path = path.as_ref();
        secure_history(path, true).map_err(HistoryError::Privacy)?;
        // Resolve trusted XDG/home ancestor aliases, while retaining NOFOLLOW
        // for the database itself. Do not reinterpret a filename as a SQLite URI.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let sqlite_path = parent.canonicalize().map_err(HistoryError::Privacy)?.join(
            path.file_name().ok_or_else(|| {
                HistoryError::Privacy(io::Error::other("Missing history filename"))
            })?,
        );
        let connection = Connection::open_with_flags(
            sqlite_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        connection.busy_timeout(Duration::from_millis(250))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage (
                 result_id TEXT PRIMARY KEY NOT NULL,
                 launch_count INTEGER NOT NULL CHECK (launch_count >= 0),
                 last_used INTEGER NOT NULL CHECK (last_used >= 0)
             );",
        )?;
        Ok(Self {
            connection,
            path: path.to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record_launch(&mut self, result_id: &str) -> Result<UsageStats, HistoryError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .min(i64::MAX as u64) as i64;
        self.record_launch_at(result_id, now)
    }

    pub fn record_launch_at(
        &mut self,
        result_id: &str,
        unix_seconds: i64,
    ) -> Result<UsageStats, HistoryError> {
        if result_id.is_empty() {
            return Ok(UsageStats::default());
        }
        let unix_seconds = unix_seconds.max(0);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO usage (result_id, launch_count, last_used)
             VALUES (?1, 1, ?2)
             ON CONFLICT(result_id) DO UPDATE SET
                 launch_count = MIN(usage.launch_count + 1, 9223372036854775807),
                 last_used = excluded.last_used",
            params![result_id, unix_seconds],
        )?;
        let stats = transaction.query_row(
            "SELECT launch_count, last_used FROM usage WHERE result_id = ?1",
            [result_id],
            |row| {
                Ok(UsageStats {
                    launch_count: row.get::<_, i64>(0)?.max(0) as u64,
                    last_used_unix_seconds: Some(row.get::<_, i64>(1)?.max(0)),
                })
            },
        )?;
        transaction.commit()?;
        Ok(stats)
    }

    pub fn stats(&self, result_id: &str) -> Result<Option<UsageStats>, HistoryError> {
        self.connection
            .query_row(
                "SELECT launch_count, last_used FROM usage WHERE result_id = ?1",
                [result_id],
                |row| {
                    Ok(UsageStats {
                        launch_count: row.get::<_, i64>(0)?.max(0) as u64,
                        last_used_unix_seconds: Some(row.get::<_, i64>(1)?.max(0)),
                    })
                },
            )
            .optional()
            .map_err(HistoryError::from)
    }

    pub fn snapshot(&self) -> Result<UsageSnapshot, HistoryError> {
        let mut statement = self
            .connection
            .prepare("SELECT result_id, launch_count, last_used FROM usage")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                UsageStats {
                    launch_count: row.get::<_, i64>(1)?.max(0) as u64,
                    last_used_unix_seconds: Some(row.get::<_, i64>(2)?.max(0)),
                },
            ))
        })?;
        let values = rows.collect::<Result<HashMap<_, _>, _>>()?;
        Ok(UsageSnapshot { values })
    }

    pub fn clear(&self) -> Result<(), HistoryError> {
        self.connection.execute("DELETE FROM usage", [])?;
        Ok(())
    }
}

fn secure_history(path: &Path, create: bool) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
    {
        if create {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        } else if let Err(error) = fs::symlink_metadata(parent) {
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(());
            }
            return Err(error);
        }
        // Open the private leaf without following a replacement symlink. Do not
        // chmod XDG ancestors: they can legitimately contain other app data.
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
            )
            .open(parent)?;
        if directory.metadata()?.uid() != rustix::process::geteuid().as_raw() {
            return Err(io::Error::other(
                "History directory must belong to the current user",
            ));
        }
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    if create {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error),
        }
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let file = Path::new(&name);
        let metadata = match fs::symlink_metadata(file) {
            Ok(metadata) => metadata,
            Err(error)
                if (!create || !suffix.is_empty()) && error.kind() == io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.nlink() != 1
        {
            return Err(io::Error::other(
                "History files must be owned regular files without links",
            ));
        }
        // No extra open/close of existing SQLite files: closing any descriptor
        // can release another connection's POSIX locks in this process. The
        // private directory excludes other UIDs; same-UID writers are trusted.
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn disabled_migration_does_not_create_new_history() {
        let root = tempdir().unwrap();
        let path = root.path().join("unused/history.sqlite3");
        UsageStore::protect_existing(&path).unwrap();
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn filename_only_keeps_working_directory_permissions() {
        if std::env::var_os("SPOTLIGHT_HISTORY_TEST_CHILD").is_some() {
            let mut store = UsageStore::open("history.sqlite3").unwrap();
            store.record_launch_at("synthetic", 1).unwrap();
            assert_eq!(mode(Path::new("history.sqlite3")), 0o600);
            assert_eq!(mode(Path::new(".")), 0o755);
            return;
        }
        let root = tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "history::tests::filename_only_keeps_working_directory_permissions"
                ])
                .env("SPOTLIGHT_HISTORY_TEST_CHILD", "1")
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn repairs_legacy_database_and_sidecars_without_losing_rows() {
        let root = tempdir().unwrap();
        let root_mode = mode(root.path());
        let directory = root.path().join("spotlight-linux");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("history.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection.execute_batch("CREATE TABLE usage (result_id TEXT PRIMARY KEY, launch_count INTEGER, last_used INTEGER); INSERT INTO usage VALUES ('application:test.desktop', 3, 100);").unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        for suffix in ["", "-wal", "-shm"] {
            fs::set_permissions(
                format!("{}{}", path.display(), suffix),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        let mut store = UsageStore::open(&path).unwrap();
        assert_eq!(mode(&directory), 0o700);
        for suffix in ["", "-wal", "-shm"] {
            assert_eq!(
                mode(Path::new(&format!("{}{}", path.display(), suffix))),
                0o600
            );
        }
        assert_eq!(
            store
                .stats("application:test.desktop")
                .unwrap()
                .unwrap()
                .launch_count,
            3
        );
        store
            .record_launch_at("application:test.desktop", 200)
            .unwrap();
        assert_eq!(
            connection
                .query_row("SELECT launch_count FROM usage", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(mode(root.path()), root_mode);
    }

    #[test]
    fn unsafe_database_directory_and_sidecars_are_rejected() {
        use std::os::unix::fs::symlink;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            for hardlink in [false, true] {
                let root = tempdir().unwrap();
                let path = root.path().join("history.sqlite3");
                let target = root.path().join("unrelated");
                fs::write(&target, b"preserve").unwrap();
                fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
                let link = PathBuf::from(format!("{}{}", path.display(), suffix));
                if hardlink {
                    fs::hard_link(&target, &link).unwrap();
                } else {
                    symlink(&target, &link).unwrap();
                }
                assert!(UsageStore::open(&path).is_err());
                assert_eq!(fs::read(&target).unwrap(), b"preserve");
                assert_eq!(mode(&target), 0o644);
            }
        }
        let root = tempdir().unwrap();
        let target = root.path().join("shared");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&target, root.path().join("private")).unwrap();
        assert!(UsageStore::open(root.path().join("private/history.sqlite3")).is_err());
        assert_eq!(mode(&target), 0o755);
        assert!(!target.join("history.sqlite3").exists());
    }

    #[test]
    fn rollback_journal_is_hardened_before_open() {
        let root = tempdir().unwrap();
        let path = root.path().join("history.sqlite3");
        let journal = root.path().join("history.sqlite3-journal");
        fs::write(&journal, []).unwrap();
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o644)).unwrap();
        secure_history(&path, true).unwrap();
        assert_eq!(mode(&journal), 0o600);
    }

    #[test]
    fn usage_is_persisted_and_snapshotted() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("history.sqlite3");
        {
            let mut store = UsageStore::open(&path).unwrap();
            store.record_launch_at("app:terminal", 100).unwrap();
            let stats = store.record_launch_at("app:terminal", 200).unwrap();
            assert_eq!(stats.launch_count, 2);
            assert_eq!(stats.last_used_unix_seconds, Some(200));
        }

        let store = UsageStore::open(&path).unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.get("app:terminal").launch_count, 2);
        assert_eq!(snapshot.len(), 1);
    }

    #[test]
    fn clear_removes_only_usage_rows() {
        let directory = tempdir().unwrap();
        let mut store = UsageStore::open(directory.path().join("history.sqlite3")).unwrap();
        store.record_launch_at("app:files", 100).unwrap();
        store.clear().unwrap();
        assert!(store.snapshot().unwrap().is_empty());
    }
}
