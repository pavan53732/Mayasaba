//! Mayasaba SQLite storage: the durable local source of truth.
//!
//! SQLite is authoritative for persisted state (AGENTS.md section 12). Nothing here decides meaning; it
//! applies the canonical schema and commits what an owning service has already decided. Domain
//! interpretation belongs to the service that owns the concept, not to this crate.
//!
//! The schema is embedded with `include_str!` rather than read from an install path. `schema.sql` stays
//! the single canonical artifact - the same file contract verification checks and the local gate enforces -
//! and embedding it means the shipped MSI carries no separate schema file that could drift from it.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

/// The canonical schema, embedded at compile time. Byte-identical to `schemas/sqlite-v1/schema.sql`.
pub const SCHEMA_SQL: &str = include_str!("../../../schemas/sqlite-v1/schema.sql");

#[derive(Debug)]
pub enum StorageError {
    /// The canonical schema could not be applied. The database is unusable.
    Schema(rusqlite::Error),
    /// A statement failed. Any transaction in progress has been rolled back.
    Db(rusqlite::Error),
    /// The requested row does not exist.
    NotFound(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Schema(e) => write!(f, "canonical schema could not be applied: {e}"),
            StorageError::Db(e) => write!(f, "storage operation failed: {e}"),
            StorageError::NotFound(what) => write!(f, "not found: {what}"),
        }
    }
}

impl std::error::Error for StorageError {}

pub type Result<T> = std::result::Result<T, StorageError>;

/// A project creation request, already validated by the owning service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub project_id: String,
    pub name: String,
    pub local_path: String,
    pub brief_id: String,
    pub brief_body: String,
    pub brief_source: String,
    pub event_id: String,
    pub created_at: String,
}

/// What actually exists after creation committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedProject {
    pub project_id: String,
    pub brief_id: String,
    pub epoch: i64,
    pub phase: String,
    pub status: String,
}

/// Authoritative readback. This is the state the Control Room must display, not a UI projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub project_id: String,
    pub name: String,
    pub local_path: String,
    pub phase: String,
    pub status: String,
    pub current_epoch: i64,
    pub brief_id: Option<String>,
    pub brief_version: Option<i64>,
    pub brief_body: Option<String>,
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open the database and apply the canonical schema.
    ///
    /// `schema.sql` uses `CREATE TABLE IF NOT EXISTS` throughout, so applying it to an existing database is
    /// additive and safe; it creates what is missing and leaves what exists alone.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = if path.as_os_str() == ":memory:" {
            Connection::open_in_memory()
        } else {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)
        }
        .map_err(StorageError::Schema)?;

        // Foreign keys are off by default in SQLite. schema.sql declares REFERENCES constraints that are
        // silently ignored unless this is enabled per connection, so a database could accept orphaned rows
        // while appearing to enforce them.
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(StorageError::Schema)?;
        conn.execute_batch(SCHEMA_SQL).map_err(StorageError::Schema)?;

        Ok(Storage { conn })
    }

    /// In-memory instance for tests.
    pub fn open_in_memory() -> Result<Self> {
        Storage::open(Path::new(":memory:"))
    }

    /// Create a project and its intent anchor as one transaction.
    ///
    /// The four writes - project row, ProjectBrief version 1, initial epoch, PROJECT_CREATED event - are a
    /// single atomic unit. A project must never exist without its brief, because the brief is the analysis
    /// anchor DEC-030 depends on. If any statement fails, all four are rolled back and this returns Err.
    ///
    /// No idempotency is claimed here. DEC-027 keys material-action idempotency by project_id + operation_id,
    /// and neither exists at creation time: the project_id is the thing being minted. Retry semantics need a
    /// contract decision defining the client-supplied key, its uniqueness scope, replay behaviour and
    /// response semantics, so this deliberately does not pretend to offer them.
    pub fn create_project(&mut self, new: &NewProject) -> Result<CreatedProject> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;

        tx.execute(
            "INSERT INTO projects (project_id, name, local_path, phase, status, current_epoch, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'DISCOVERY', 'ACTIVE', 0, ?4, ?4)",
            rusqlite::params![new.project_id, new.name, new.local_path, new.created_at],
        )
        .map_err(StorageError::Db)?;

        tx.execute(
            "INSERT INTO project_briefs (brief_id, project_id, version, body, source, supersedes_brief_id, created_at)
             VALUES (?1, ?2, 1, ?3, ?4, NULL, ?5)",
            rusqlite::params![new.brief_id, new.project_id, new.brief_body, new.brief_source, new.created_at],
        )
        .map_err(StorageError::Db)?;

        tx.execute(
            "INSERT INTO project_epochs (project_id, epoch, reason, created_at) VALUES (?1, 0, 'PROJECT_CREATED', ?2)",
            rusqlite::params![new.project_id, new.created_at],
        )
        .map_err(StorageError::Db)?;

        tx.execute(
            "INSERT INTO events (event_id, project_id, event_type, sequence, epoch, payload_json, prev_hash, event_hash, created_at)
             VALUES (?1, ?2, 'PROJECT_CREATED', 1, 0, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                new.event_id,
                new.project_id,
                format!(
                    r#"{{"brief_id":"{}","epoch":0,"local_path":"{}"}}"#,
                    new.brief_id, new.local_path
                ),
                // events.prev_hash is NOT NULL, so the genesis event carries a sentinel rather than NULL. The
                // per-project SHA-256 chain of DEC-034 begins properly when that lands; this slice records a
                // deterministic placeholder rather than pretending the chain is already enforced.
                "genesis",
                format!("genesis:{}", new.event_id),
                new.created_at,
            ],
        )
        .map_err(StorageError::Db)?;

        tx.commit().map_err(StorageError::Db)?;

        Ok(CreatedProject {
            project_id: new.project_id.clone(),
            brief_id: new.brief_id.clone(),
            epoch: 0,
            phase: "DISCOVERY".to_string(),
            status: "ACTIVE".to_string(),
        })
    }

    /// Authoritative project readback, including the current brief.
    pub fn get_project(&self, project_id: &str) -> Result<ProjectRecord> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.project_id, p.name, p.local_path, p.phase, p.status, p.current_epoch,
                        (SELECT b.brief_id      FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1),
                        (SELECT b.version       FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1),
                        (SELECT b.body          FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1)
                 FROM projects p WHERE p.project_id = ?1",
            )
            .map_err(StorageError::Db)?;

        let row = stmt
            .query_row([project_id], |r| {
                Ok(ProjectRecord {
                    project_id: r.get(0)?,
                    name: r.get(1)?,
                    local_path: r.get(2)?,
                    phase: r.get(3)?,
                    status: r.get(4)?,
                    current_epoch: r.get(5)?,
                    brief_id: r.get(6)?,
                    brief_version: r.get(7)?,
                    brief_body: r.get(8)?,
                })
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound(format!("project {project_id}")),
                other => StorageError::Db(other),
            })?;

        Ok(row)
    }

    /// Count rows in a table. Used by tests to assert that a failed transaction wrote nothing.
    pub fn count(&self, table: &str) -> Result<i64> {
        // The table name comes from test code and this crate's own constants, never from user input.
        let sql = format!("SELECT COUNT(*) FROM {table}");
        self.conn
            .query_row(&sql, [], |r| r.get(0))
            .map_err(StorageError::Db)
    }

    /// Apply a fault-injection trigger. Used only by tests, to make a real SQLite statement fail inside a
    /// transaction so rollback is proven by the database rather than asserted by a mock.
    pub fn inject_fault_before_insert(&self, table: &str) -> Result<()> {
        let sql = format!(
            "CREATE TRIGGER IF NOT EXISTS inject_fault_{table} BEFORE INSERT ON {table}
             BEGIN SELECT RAISE(ABORT, 'injected fault'); END;"
        );
        self.conn.execute_batch(&sql).map_err(StorageError::Db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}