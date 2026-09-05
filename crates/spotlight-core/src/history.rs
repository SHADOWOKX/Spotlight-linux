use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
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
    pub fn open(path: impl AsRef<Path>) -> Result<Self, HistoryError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(HistoryError::CreateDirectory)?;
        }
        let connection = Connection::open(path)?;
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

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
