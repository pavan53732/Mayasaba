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
    /// A stored value cannot be read back as the type its column promises.
    ///
    /// Every column this crate writes is written from a closed vocabulary - a `CHECK` constraint in
    /// `schema.sql` enforces most of them - so a value that does not parse means the row was written outside
    /// this crate or the vocabulary changed without a migration. Reporting it beats returning a default that
    /// would look like a recorded fact.
    Malformed { column: String, detail: String },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Schema(e) => write!(f, "canonical schema could not be applied: {e}"),
            StorageError::Db(e) => write!(f, "storage operation failed: {e}"),
            StorageError::NotFound(what) => write!(f, "not found: {what}"),
            StorageError::Malformed { column, detail } => {
                write!(
                    f,
                    "stored value in `{column}` cannot be read back: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for StorageError {}

pub type Result<T> = std::result::Result<T, StorageError>;

/// Display name used when a workspace has no usable leaf name.
///
/// A filesystem root such as `C:\` has no folder name of its own. Rather than inventing something that
/// pretends to be the folder's name, or persisting an empty name, a workspace root gets an explicit label.
pub const ROOT_WORKSPACE_NAME: &str = "Local Workspace";

/// Derive a project's initial display name from its canonical workspace path.
///
/// This lives here, at the lowest layer both the workspace crate and this crate can see, for a reason that
/// is about enforcement rather than convenience. The name is a pure function of the canonical path, so if
/// the workspace crate derived it *and* this crate accepted a caller-supplied name, there would be two
/// derivations and an injection surface: any crate depending on `mayasaba-storage` could persist a project
/// whose name disagreed with its folder, bypassing the owning service entirely.
///
/// Deriving it once, here, and using it in both places removes the surface structurally rather than by
/// convention. `projects.name` therefore cannot be set to anything other than the workspace folder's leaf
/// name through this crate (DEC-050).
///
/// The display name is metadata, not identity: `project_id` is generated independently and is never derived
/// from a path. Two projects may therefore begin with the same display name without colliding.
pub fn derive_project_display_name(canonical_path: &str) -> String {
    use std::path::{Component, Path};

    let path = Path::new(canonical_path);

    // `C:\` canonicalizes with a trailing separator and `Path::file_name` returns None for a root, so fall
    // back to the last normal component. `C:\Users` still yields `Users`.
    let leaf = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| {
            path.components()
                .filter_map(|c| match c {
                    Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                    _ => None,
                })
                .next_back()
        });

    match leaf {
        Some(name) if !name.trim().is_empty() => name,
        _ => ROOT_WORKSPACE_NAME.to_string(),
    }
}

/// A project creation request, already validated by the owning service.
///
/// There is deliberately no `name` field. The display name is derived from `local_path` below, so no caller
/// can inject one. `local_path` must already be the canonical form the owning service validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub project_id: String,
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
    pub created_at: String,
}

pub struct Storage {
    conn: Connection,
}

/// A blast radius, as `council_mode_selections.inputs_json` records it.
///
/// `scope_roots` is written already canonicalized - sorted and de-duplicated - because the selection id is
/// derived from it and two spellings of one scope set must not produce two ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilBlastRadius {
    pub scope_roots: Vec<String>,
    pub affected_file_count: u64,
    pub crosses_workspace_boundary: bool,
}

/// A `council_mode_selections` row to insert.
///
/// The JSON columns are assembled here rather than accepted as strings, so a caller cannot persist a
/// `reasons_json` that is not the JSON array the contract declares, and so no consumer of this crate has to
/// write JSON to persist a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCouncilModeSelection {
    pub selection_id: String,
    pub project_id: String,
    pub round_id: Option<String>,
    pub decision_class: String,
    pub mode: String,
    pub blast_radius: CouncilBlastRadius,
    pub prior_validation_failures: u64,
    pub open_disputes: u64,
    pub reasons: Vec<String>,
    pub selector_version: String,
    pub override_source: String,
    pub supersedes_selection_id: Option<String>,
    pub created_at: String,
}

/// A `council_mode_selections` row as stored.
///
/// `inputs_json` and `reasons_json` are returned as the exact stored text. `reasons_json` is also decoded,
/// because every reader wants the reasons; `inputs_json` is not, because a caller that needs the inputs
/// re-derives them from authoritative facts rather than trusting a recorded copy, and inventing a second
/// decoder for it would risk the two disagreeing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilModeSelectionRecord {
    pub selection_id: String,
    pub project_id: String,
    pub round_id: Option<String>,
    pub decision_class: String,
    pub mode: String,
    pub inputs_json: String,
    pub reasons_json: String,
    pub reasons: Vec<String>,
    pub selector_version: String,
    pub override_source: String,
    pub supersedes_selection_id: Option<String>,
    pub created_at: String,
}

/// A `council_round_roles` row to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCouncilRoundRole {
    pub round_id: String,
    pub agent_id: String,
    pub role: String,
    pub assigned_reason: String,
    pub assigned_at: String,
}

/// A `council_round_roles` row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilRoundRoleRecord {
    pub round_id: String,
    pub agent_id: String,
    pub role: String,
    pub assigned_reason: String,
    pub assigned_at: String,
}

/// One resolved or unresolved reference in a claim's basis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilClaimReference {
    pub reference: String,
    pub reason: String,
}

/// The basis a computed claim grade rests on, as `council_claim_grades.basis_json` records it.
///
/// This is stored so a grade is explainable months later without re-resolving against a repository that has
/// since changed. It is evidence of how the grade was reached, never an input to computing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilClaimGradeBasis {
    pub claim_id: String,
    pub reason: String,
    pub resolved: Vec<CouncilClaimReference>,
    pub unresolved: Vec<CouncilClaimReference>,
}

/// A `council_claim_grades` row to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCouncilClaimGrade {
    pub claim_id: String,
    pub position_id: String,
    pub round_id: String,
    pub grade: String,
    pub load_bearing: bool,
    pub basis: CouncilClaimGradeBasis,
    pub computed_at: String,
}

/// A `council_claim_grades` row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilClaimGradeRecord {
    pub claim_id: String,
    pub position_id: String,
    pub round_id: String,
    pub grade: String,
    pub load_bearing: bool,
    /// The exact stored basis text, for a caller that audits the record.
    pub basis_json: String,
    pub computed_at: String,
}

/// A `council_budget_ledger` row to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCouncilBudgetLedgerEntry {
    pub entry_id: String,
    pub council_session_id: Option<String>,
    pub round_id: Option<String>,
    pub kind: String,
    /// `None` is the schema's `NULL` amount. It is required - not merely permitted - for an `UNAVAILABLE`
    /// token count, because an unavailable count must never carry a number.
    pub amount: Option<i64>,
    pub availability: String,
    pub detail: Option<String>,
    pub recorded_at: String,
}

/// A `council_budget_ledger` row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilBudgetLedgerRecord {
    pub entry_id: String,
    pub council_session_id: Option<String>,
    pub round_id: Option<String>,
    pub kind: String,
    pub amount: Option<i64>,
    pub availability: String,
    pub detail: Option<String>,
    pub recorded_at: String,
}

/// A `council_outcome_agent_links` row to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCouncilOutcomeAgentLink {
    pub agent_id: String,
    pub position_id: Option<String>,
    pub stance: String,
}

/// A `council_decision_outcomes` row plus its links, to insert as one transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCouncilDecisionOutcome {
    pub outcome_record_id: String,
    pub decision_id: String,
    pub council_session_id: Option<String>,
    pub round_id: Option<String>,
    pub mode: String,
    pub decision_class: String,
    pub status: String,
    pub validation_evidence_id: Option<String>,
    pub source: String,
    pub supersedes_outcome_id: Option<String>,
    pub recorded_at: String,
    pub agent_links: Vec<NewCouncilOutcomeAgentLink>,
}

/// A `council_decision_outcomes` row as stored. The stored record has no `informational_only` column: the
/// contract pins it to `true`, so storing it would store a value that cannot vary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilDecisionOutcomeRecord {
    pub outcome_record_id: String,
    pub decision_id: String,
    pub council_session_id: Option<String>,
    pub round_id: Option<String>,
    pub mode: String,
    pub decision_class: String,
    pub status: String,
    pub validation_evidence_id: Option<String>,
    pub source: String,
    pub supersedes_outcome_id: Option<String>,
    pub recorded_at: String,
}

/// A `council_outcome_agent_links` row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilOutcomeAgentLinkRecord {
    pub outcome_record_id: String,
    pub agent_id: String,
    pub position_id: Option<String>,
    pub stance: String,
}

/// A durable inconsistency found during startup recovery.
///
/// Recovery never repairs silently. It reports, because repairing authoritative project state is an owning
/// service's decision, not something storage may take on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryIssue {
    pub kind: &'static str,
    pub detail: String,
}

/// Result of the startup recovery scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Verdict from SQLite's own integrity check.
    pub integrity_ok: bool,
    /// Every inconsistency found, in a stable order.
    pub issues: Vec<RecoveryIssue>,
}

impl RecoveryReport {
    /// True when the durable state needs no attention.
    pub fn is_clean(&self) -> bool {
        self.integrity_ok && self.issues.is_empty()
    }
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
            Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            )
        }
        .map_err(StorageError::Schema)?;

        // Foreign keys are off by default in SQLite. schema.sql declares REFERENCES constraints that are
        // silently ignored unless this is enabled per connection, so a database could accept orphaned rows
        // while appearing to enforce them.
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(StorageError::Schema)?;
        conn.execute_batch(SCHEMA_SQL)
            .map_err(StorageError::Schema)?;

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
            rusqlite::params![
                new.project_id,
                // Derived here from the canonical workspace path, not accepted from the caller. See
                // derive_project_display_name for why this lives at this layer.
                derive_project_display_name(&new.local_path),
                new.local_path,
                new.created_at,
            ],
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
                        (SELECT b.body          FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1),
                        p.created_at
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
                    created_at: r.get(9)?,
                })
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound(format!("project {project_id}"))
                }
                other => StorageError::Db(other),
            })?;

        Ok(row)
    }

    /// Every project, newest first.
    ///
    /// Ordering is explicit rather than left to the engine: the Control Room's list must not depend on row
    /// insertion order or on a planner's whim. Each row carries the current brief, so the list is a single
    /// projection an implementation can render directly.
    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.project_id, p.name, p.local_path, p.phase, p.status, p.current_epoch,
                        (SELECT b.brief_id      FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1),
                        (SELECT b.version       FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1),
                        (SELECT b.body          FROM project_briefs b WHERE b.project_id = p.project_id ORDER BY b.version DESC LIMIT 1),
                        p.created_at
                 FROM projects p
                 ORDER BY p.created_at DESC, p.project_id ASC",
            )
            .map_err(StorageError::Db)?;

        let rows = stmt
            .query_map([], |r| {
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
                    created_at: r.get(9)?,
                })
            })
            .map_err(StorageError::Db)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
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

    /// Startup recovery scan.
    ///
    /// Runs SQLite's own integrity check and then looks for state that atomic creation should make impossible
    /// but that a crash, an older schema, or a connection with foreign keys disabled could still produce. It
    /// reports; it never repairs.
    pub fn recover(&self) -> Result<RecoveryReport> {
        let integrity: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(StorageError::Db)?;
        let integrity_ok = integrity.eq_ignore_ascii_case("ok");

        let mut issues = Vec::new();
        if !integrity_ok {
            issues.push(RecoveryIssue {
                kind: "SQLITE_INTEGRITY",
                detail: integrity.clone(),
            });
        }

        // A project must always have a brief: the brief is the intent anchor, and creation commits the two
        // together. A project without one means the invariant was broken outside the atomic path.
        let missing_brief = self.string_column(
            "SELECT p.project_id FROM projects p
                 WHERE NOT EXISTS (SELECT 1 FROM project_briefs b WHERE b.project_id = p.project_id)
                 ORDER BY p.project_id",
        )?;
        for id in missing_brief {
            issues.push(RecoveryIssue {
                kind: "PROJECT_WITHOUT_BRIEF",
                detail: format!("project {id} has no ProjectBrief; its intent anchor is missing"),
            });
        }

        // The summary epoch must match the newest epoch row. A mismatch means the column and the history
        // disagree, so a context digest computed from one would be wrong for the other.
        let mut drift = self
            .conn
            .prepare(
                "SELECT p.project_id, p.current_epoch,
                        COALESCE((SELECT MAX(e.epoch) FROM project_epochs e WHERE e.project_id = p.project_id), -1)
                 FROM projects p ORDER BY p.project_id",
            )
            .map_err(StorageError::Db)?;
        let rows = drift
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(StorageError::Db)?;
        for row in rows {
            let (id, current, newest) = row.map_err(StorageError::Db)?;
            if newest != current {
                issues.push(RecoveryIssue {
                    kind: "EPOCH_SUMMARY_DRIFT",
                    detail: format!(
                        "project {id} reports epoch {current} but its newest epoch row is {newest}"
                    ),
                });
            }
        }

        // Foreign keys are enforced per connection, so rows written while they were off can survive.
        let orphans: i64 = self
            .conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM project_briefs b WHERE NOT EXISTS (SELECT 1 FROM projects p WHERE p.project_id = b.project_id))
                      + (SELECT COUNT(*) FROM project_epochs e WHERE NOT EXISTS (SELECT 1 FROM projects p WHERE p.project_id = e.project_id))",
                [],
                |r| r.get(0),
            )
            .map_err(StorageError::Db)?;
        if orphans > 0 {
            issues.push(RecoveryIssue {
                kind: "ORPHANED_PROJECT_ROWS",
                detail: format!(
                    "{orphans} brief or epoch row(s) reference a project that does not exist"
                ),
            });
        }

        Ok(RecoveryReport {
            integrity_ok,
            issues,
        })
    }

    fn string_column(&self, sql: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(sql).map_err(StorageError::Db)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Insert a `council_mode_selections` row (DEC-052).
    ///
    /// The JSON columns are assembled here from typed fields, so no caller can persist a `reasons_json` that
    /// is not the JSON array the contract declares.
    ///
    /// There is deliberately no update or delete for this table. Escalation appends a record whose
    /// `supersedes_selection_id` names the previous one, because the contract makes the chain append-only and
    /// a silent in-place escalation would erase the record of what the controller first computed.
    pub fn insert_council_mode_selection(&mut self, new: &NewCouncilModeSelection) -> Result<()> {
        let mode = require_vocabulary("mode", &new.mode, MODE_VOCABULARY)?;
        let decision_class = require_vocabulary(
            "decision_class",
            &new.decision_class,
            DECISION_CLASS_VOCABULARY,
        )?;
        let override_source = require_vocabulary(
            "override_source",
            &new.override_source,
            OVERRIDE_SOURCE_VOCABULARY,
        )?;

        let inputs_json = mode_inputs_json(
            &new.blast_radius,
            new.prior_validation_failures,
            new.open_disputes,
        );
        let reasons_json = encode_json_string_array(&new.reasons);

        self.conn
            .execute(
                "INSERT INTO council_mode_selections
                   (selection_id, project_id, round_id, decision_class, mode, inputs_json, reasons_json,
                    selector_version, override_source, supersedes_selection_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    new.selection_id,
                    new.project_id,
                    new.round_id,
                    decision_class,
                    mode,
                    inputs_json,
                    reasons_json,
                    new.selector_version,
                    override_source,
                    new.supersedes_selection_id,
                    new.created_at,
                ],
            )
            .map_err(StorageError::Db)?;
        Ok(())
    }

    /// Every mode selection recorded for a project, oldest first.
    ///
    /// Ordered by `created_at` then `selection_id`, so an append-only supersession chain reads in the order
    /// it was recorded and two records sharing an instant still order deterministically.
    pub fn list_council_mode_selections(
        &self,
        project_id: &str,
    ) -> Result<Vec<CouncilModeSelectionRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT selection_id, project_id, round_id, decision_class, mode, inputs_json, reasons_json,
                        selector_version, override_source, supersedes_selection_id, created_at
                 FROM council_mode_selections
                 WHERE project_id = ?1
                 ORDER BY created_at ASC, selection_id ASC",
            )
            .map_err(StorageError::Db)?;

        let rows = stmt
            .query_map([project_id], |r| {
                Ok(CouncilModeSelectionRecord {
                    selection_id: r.get(0)?,
                    project_id: r.get(1)?,
                    round_id: r.get(2)?,
                    decision_class: r.get(3)?,
                    mode: r.get(4)?,
                    inputs_json: r.get(5)?,
                    reasons_json: r.get(6)?,
                    // Decoded separately below: `reasons_json` is returned verbatim as well, so a reader can
                    // see the stored text and the decoded reasons side by side when they disagree.
                    reasons: Vec::new(),
                    selector_version: r.get(7)?,
                    override_source: r.get(8)?,
                    supersedes_selection_id: r.get(9)?,
                    created_at: r.get(10)?,
                })
            })
            .map_err(StorageError::Db)?;

        let mut records: Vec<CouncilModeSelectionRecord> = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)?;
        for record in records.iter_mut() {
            record.reasons = decode_json_string_array(&record.reasons_json).map_err(|detail| {
                StorageError::Malformed {
                    column: "reasons_json".to_string(),
                    detail,
                }
            })?;
        }
        Ok(records)
    }

    /// Insert a `council_round_roles` row.
    ///
    /// The table's PRIMARY KEY is `(round_id, agent_id, role)`, so the same agent may hold two duties but
    /// never the same one twice. A repeated insert is refused by the key rather than silently ignored; the
    /// controller knows which agents hold which duty and should not be guessing.
    pub fn insert_council_round_role(&mut self, new: &NewCouncilRoundRole) -> Result<()> {
        let role = require_vocabulary("role", &new.role, ROLE_VOCABULARY)?;
        self.conn
            .execute(
                "INSERT INTO council_round_roles (round_id, agent_id, role, assigned_reason, assigned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![new.round_id, new.agent_id, role, new.assigned_reason, new.assigned_at],
            )
            .map_err(StorageError::Db)?;
        Ok(())
    }

    /// The roles assigned in a round, in `role` then `agent_id` order.
    pub fn list_council_round_roles(&self, round_id: &str) -> Result<Vec<CouncilRoundRoleRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT round_id, agent_id, role, assigned_reason, assigned_at
                 FROM council_round_roles
                 WHERE round_id = ?1
                 ORDER BY role ASC, agent_id ASC",
            )
            .map_err(StorageError::Db)?;

        let rows = stmt
            .query_map([round_id], |r| {
                Ok(CouncilRoundRoleRecord {
                    round_id: r.get(0)?,
                    agent_id: r.get(1)?,
                    role: r.get(2)?,
                    assigned_reason: r.get(3)?,
                    assigned_at: r.get(4)?,
                })
            })
            .map_err(StorageError::Db)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }

    /// Insert a `council_claim_grades` row: one controller-computed grade for one claim.
    ///
    /// There is no delete. A grade is a computed historical fact about what the controller found at a given
    /// instant; recomputing it produces a new claim id, not a replacement for this row.
    pub fn insert_council_claim_grade(&mut self, new: &NewCouncilClaimGrade) -> Result<()> {
        let grade = require_vocabulary("grade", &new.grade, CLAIM_GRADE_VOCABULARY)?;
        let basis_json = claim_grade_basis_json(&new.basis);

        self.conn
            .execute(
                "INSERT INTO council_claim_grades
                   (claim_id, position_id, round_id, grade, load_bearing, basis_json, computed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    new.claim_id,
                    new.position_id,
                    new.round_id,
                    grade,
                    if new.load_bearing { 1_i64 } else { 0_i64 },
                    basis_json,
                    new.computed_at,
                ],
            )
            .map_err(StorageError::Db)?;
        Ok(())
    }

    /// The graded claims of one position, in `claim_id` order.
    ///
    /// This is the read a position's weakest-load-bearing-claim grade is reconstructed from, so it must
    /// return every claim including the supporting ones: excluding them would hide which claims were
    /// considered and which were not.
    pub fn list_council_claim_grades(
        &self,
        position_id: &str,
    ) -> Result<Vec<CouncilClaimGradeRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT claim_id, position_id, round_id, grade, load_bearing, basis_json, computed_at
                 FROM council_claim_grades
                 WHERE position_id = ?1
                 ORDER BY claim_id ASC",
            )
            .map_err(StorageError::Db)?;

        let rows = stmt
            .query_map([position_id], |r| {
                Ok(CouncilClaimGradeRecord {
                    claim_id: r.get(0)?,
                    position_id: r.get(1)?,
                    round_id: r.get(2)?,
                    grade: r.get(3)?,
                    // `load_bearing` is constrained to 0 or 1 by a CHECK, so any other value would be a row
                    // written outside this crate. Reading it as `!= 0` keeps that from panicking while still
                    // reporting the common case correctly.
                    load_bearing: r.get::<_, i64>(4)? != 0,
                    basis_json: r.get(5)?,
                    computed_at: r.get(6)?,
                })
            })
            .map_err(StorageError::Db)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }

    /// Insert a `council_budget_ledger` entry.
    ///
    /// An `UNAVAILABLE` entry must carry no amount: this refuses a number rather than ignoring it, because a
    /// recorded estimate for an unreported token count is exactly the invented number the policy forbids and
    /// it would be indistinguishable from a report once stored.
    ///
    /// There is no update and no delete. A pause is a `PAUSED` entry plus its matching `RESUMED` entry, which
    /// is how wall-clock excludes the interval without any row being rewritten.
    pub fn insert_council_budget_entry(&mut self, new: &NewCouncilBudgetLedgerEntry) -> Result<()> {
        let kind = require_vocabulary("kind", &new.kind, BUDGET_KIND_VOCABULARY)?;
        let availability = require_vocabulary(
            "availability",
            &new.availability,
            BUDGET_AVAILABILITY_VOCABULARY,
        )?;

        if availability == "UNAVAILABLE" && new.amount.is_some() {
            return Err(StorageError::Malformed {
                column: "council_budget_ledger.amount".to_string(),
                detail: "an UNAVAILABLE entry must carry no amount; an unreported count is never estimated"
                    .to_string(),
            });
        }

        self.conn
            .execute(
                "INSERT INTO council_budget_ledger
                   (entry_id, council_session_id, round_id, kind, amount, availability, detail_json, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    new.entry_id,
                    new.council_session_id,
                    new.round_id,
                    kind,
                    new.amount,
                    availability,
                    new.detail,
                    new.recorded_at,
                ],
            )
            .map_err(StorageError::Db)?;
        Ok(())
    }

    /// The ledger for one council session, in `recorded_at` then `entry_id` order.
    ///
    /// Entries with no council session are included, because a round-scoped entry written before the session
    /// id was known still belongs to the council's consumption. Ordering is the insertion order the interval
    /// walk in `crates/council` depends on: a `PAUSED` entry must precede its `RESUMED` entry.
    pub fn list_council_budget_ledger(
        &self,
        council_session_id: &str,
    ) -> Result<Vec<CouncilBudgetLedgerRecord>> {
        self.query_budget_ledger(
            "SELECT entry_id, council_session_id, round_id, kind, amount, availability, detail_json, recorded_at
             FROM council_budget_ledger
             WHERE council_session_id = ?1 OR council_session_id IS NULL
             ORDER BY recorded_at ASC, entry_id ASC",
            rusqlite::params![council_session_id],
        )
    }

    /// The ledger for one round, in `recorded_at` then `entry_id` order.
    ///
    /// Round-scoped and session-scoped entries are both returned: a round's consumption happens inside its
    /// session, and a view that dropped the session rows would under-report paused time.
    pub fn list_council_budget_ledger_for_round(
        &self,
        round_id: &str,
    ) -> Result<Vec<CouncilBudgetLedgerRecord>> {
        self.query_budget_ledger(
            "SELECT entry_id, council_session_id, round_id, kind, amount, availability, detail_json, recorded_at
             FROM council_budget_ledger
             WHERE round_id = ?1 OR round_id IS NULL
             ORDER BY recorded_at ASC, entry_id ASC",
            rusqlite::params![round_id],
        )
    }

    fn query_budget_ledger(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<CouncilBudgetLedgerRecord>> {
        let mut stmt = self.conn.prepare(sql).map_err(StorageError::Db)?;
        let rows = stmt
            .query_map(params, |r| {
                Ok(CouncilBudgetLedgerRecord {
                    entry_id: r.get(0)?,
                    council_session_id: r.get(1)?,
                    round_id: r.get(2)?,
                    kind: r.get(3)?,
                    amount: r.get(4)?,
                    availability: r.get(5)?,
                    detail: r.get(6)?,
                    recorded_at: r.get(7)?,
                })
            })
            .map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }

    /// Insert a `council_decision_outcomes` row and its agent links as one transaction.
    ///
    /// **There is no update and no delete for this table, and this crate exposes none.** An outcome record is
    /// append-only by construction: an amendment or a reversal appends a record whose
    /// `supersedes_outcome_id` names its predecessor, so history is preserved and the current outcome is the
    /// newest record. `status` may not move from `HELD` to `REVERSED` by rewriting a row.
    ///
    /// The record and its links commit together, because a record whose links were partially written would
    /// attribute a stance to some agents and silently omit others.
    ///
    /// `HELD` without validation evidence is refused by the table's own `CHECK` constraint; the owning service
    /// is expected to refuse it earlier, in its own terms, rather than reaching for the database's error.
    pub fn insert_council_decision_outcome(
        &mut self,
        new: &NewCouncilDecisionOutcome,
    ) -> Result<()> {
        let mode = require_vocabulary("mode", &new.mode, MODE_VOCABULARY)?;
        let decision_class = require_vocabulary(
            "decision_class",
            &new.decision_class,
            DECISION_CLASS_VOCABULARY,
        )?;
        let status = require_vocabulary("status", &new.status, OUTCOME_STATUS_VOCABULARY)?;
        let source = require_vocabulary("source", &new.source, OUTCOME_SOURCE_VOCABULARY)?;

        let tx = self.conn.transaction().map_err(StorageError::Db)?;

        tx.execute(
            "INSERT INTO council_decision_outcomes
               (outcome_record_id, decision_id, council_session_id, round_id, mode, decision_class, status,
                validation_evidence_id, source, supersedes_outcome_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                new.outcome_record_id,
                new.decision_id,
                new.council_session_id,
                new.round_id,
                mode,
                decision_class,
                status,
                new.validation_evidence_id,
                source,
                new.supersedes_outcome_id,
                new.recorded_at,
            ],
        )
        .map_err(StorageError::Db)?;

        for link in &new.agent_links {
            tx.execute(
                "INSERT INTO council_outcome_agent_links (outcome_record_id, agent_id, position_id, stance)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![new.outcome_record_id, link.agent_id, link.position_id, link.stance],
            )
            .map_err(StorageError::Db)?;
        }

        tx.commit().map_err(StorageError::Db)?;
        Ok(())
    }

    /// Every outcome recorded for a decision, oldest first.
    ///
    /// The whole history is returned rather than only the newest record, because supersession appends: what a
    /// decision's outcome *is* now can only be read as the newest record in this list.
    pub fn list_council_decision_outcomes(
        &self,
        decision_id: &str,
    ) -> Result<Vec<CouncilDecisionOutcomeRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT outcome_record_id, decision_id, council_session_id, round_id, mode, decision_class,
                        status, validation_evidence_id, source, supersedes_outcome_id, recorded_at
                 FROM council_decision_outcomes
                 WHERE decision_id = ?1
                 ORDER BY recorded_at ASC, outcome_record_id ASC",
            )
            .map_err(StorageError::Db)?;

        let rows = stmt
            .query_map([decision_id], |r| {
                Ok(CouncilDecisionOutcomeRecord {
                    outcome_record_id: r.get(0)?,
                    decision_id: r.get(1)?,
                    council_session_id: r.get(2)?,
                    round_id: r.get(3)?,
                    mode: r.get(4)?,
                    decision_class: r.get(5)?,
                    status: r.get(6)?,
                    validation_evidence_id: r.get(7)?,
                    source: r.get(8)?,
                    supersedes_outcome_id: r.get(9)?,
                    recorded_at: r.get(10)?,
                })
            })
            .map_err(StorageError::Db)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }

    /// The agent links of one outcome record, in `agent_id` order.
    pub fn list_council_outcome_agent_links(
        &self,
        outcome_record_id: &str,
    ) -> Result<Vec<CouncilOutcomeAgentLinkRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT outcome_record_id, agent_id, position_id, stance
                 FROM council_outcome_agent_links
                 WHERE outcome_record_id = ?1
                 ORDER BY agent_id ASC",
            )
            .map_err(StorageError::Db)?;

        let rows = stmt
            .query_map([outcome_record_id], |r| {
                Ok(CouncilOutcomeAgentLinkRecord {
                    outcome_record_id: r.get(0)?,
                    agent_id: r.get(1)?,
                    position_id: r.get(2)?,
                    stance: r.get(3)?,
                })
            })
            .map_err(StorageError::Db)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }
}

/// The closed vocabularies the council tables' `CHECK` constraints enforce.
///
/// They are repeated here as the values this crate is willing to send. A `CHECK` violation would surface as
/// an opaque SQLite error; naming the vocabulary in one place means the failure is reported as a value
/// outside a known set, which is the diagnosable version of the same refusal.
const DECISION_CLASS_VOCABULARY: &[&str] = &[
    "ARCHITECTURE",
    "STACK_TECHNOLOGY",
    "IRREVERSIBLE",
    "SECURITY",
    "DATA_LOSS",
    "ROUTINE",
];
const MODE_VOCABULARY: &[&str] = &["SOLO", "REVIEW", "FULL"];
const OVERRIDE_SOURCE_VOCABULARY: &[&str] = &["NONE", "USER"];
const ROLE_VOCABULARY: &[&str] = &["PROPOSER", "SKEPTIC", "VERIFIER"];
const CLAIM_GRADE_VOCABULARY: &[&str] = &["ASSUMPTION", "CITED", "VERIFIED"];
const BUDGET_KIND_VOCABULARY: &[&str] = &[
    "ROUND_OPENED",
    "PAUSED",
    "RESUMED",
    "SPIKE_EXECUTED",
    "TOKENS_REPORTED",
    "BUDGET_EXHAUSTED",
    "SEALED",
];
const BUDGET_AVAILABILITY_VOCABULARY: &[&str] = &["REPORTED", "UNAVAILABLE"];
const OUTCOME_STATUS_VOCABULARY: &[&str] = &["HELD", "AMENDED", "REVERSED", "UNRESOLVED"];
const OUTCOME_SOURCE_VOCABULARY: &[&str] =
    &["VALIDATION_RESULT", "REOPEN_DECISION", "USER_SUPERSESSION"];

/// Refuse a value outside a closed vocabulary, naming the column and the value.
///
/// The returned reference borrows `allowed`, which is always one of this crate's own `const` tables, so the
/// caller gets the canonical spelling rather than the string it passed in.
fn require_vocabulary<'a>(column: &str, value: &str, allowed: &'a [&'a str]) -> Result<&'a str> {
    match allowed.iter().find(|candidate| **candidate == value) {
        Some(matched) => Ok(matched),
        None => Err(StorageError::Malformed {
            column: column.to_string(),
            detail: format!("`{value}` is not one of {}", allowed.join(", ")),
        }),
    }
}

/// The `inputs_json` object for a mode selection, built from typed fields.
fn mode_inputs_json(
    blast_radius: &CouncilBlastRadius,
    prior_validation_failures: u64,
    open_disputes: u64,
) -> String {
    let roots: Vec<String> = blast_radius
        .scope_roots
        .iter()
        .map(|root| json_string(root))
        .collect();
    format!(
        r#"{{"blast_radius":{{"scope_roots":[{}],"affected_file_count":{},"crosses_workspace_boundary":{}}},"prior_validation_failures":{},"open_disputes":{}}}"#,
        roots.join(","),
        blast_radius.affected_file_count,
        blast_radius.crosses_workspace_boundary,
        prior_validation_failures,
        open_disputes,
    )
}

/// The `basis_json` object for a claim grade.
fn claim_grade_basis_json(basis: &CouncilClaimGradeBasis) -> String {
    let references = |items: &[CouncilClaimReference]| -> String {
        let rendered: Vec<String> = items
            .iter()
            .map(|item| {
                format!(
                    r#"{{"reference":{},"reason":{}}}"#,
                    json_string(&item.reference),
                    json_string(&item.reason)
                )
            })
            .collect();
        rendered.join(",")
    };

    format!(
        r#"{{"claim_id":{},"reason":{},"resolved":[{}],"unresolved":[{}]}}"#,
        json_string(&basis.claim_id),
        json_string(&basis.reason),
        references(&basis.resolved),
        references(&basis.unresolved),
    )
}

/// Encode a string slice as a JSON array.
///
/// The repository has no JSON writer at this layer on purpose - `crates/protocol` owns protocol JSON and
/// `crates/storage` never interprets domain meaning - so the two shapes this crate has to persist are written
/// here, from typed input, and are deliberately not a general JSON facility.
fn encode_json_string_array(values: &[String]) -> String {
    let encoded: Vec<String> = values.iter().map(|value| json_string(value)).collect();
    format!("[{}]", encoded.join(","))
}

/// A JSON string literal, escaped per RFC 8259.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // Every other control character must be escaped; anything above U+001F may appear literally,
            // including non-ASCII, which JSON permits.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Decode a JSON array of strings, for readback.
///
/// The accepted grammar is exactly what `encode_json_string_array` emits plus the legal whitespace, escapes
/// and `\uXXXX` forms a conforming encoder may produce. Anything else is an error rather than a guess: a
/// permissive decoder would let a hand-edited row load as a plausible record.
fn decode_json_string_array(text: &str) -> std::result::Result<Vec<String>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parser = JsonArrayParser {
        chars: &chars,
        position: 0,
    };

    parser.skip_whitespace();
    parser.expect('[')?;
    let mut values = Vec::new();

    // One loop covers the legal empty array: the first iteration either reads a string or stops at `]`. The
    // success path breaks without advancing past the closing bracket, which keeps the trailing-content check
    // below honest. A comma is only legal as a separator, so a trailing comma is refused rather than read as
    // an empty final element.
    let mut after_comma = false;
    loop {
        parser.skip_whitespace();
        match parser.peek() {
            Some(']') if !after_comma => break,
            Some(']') => return Err("trailing comma before `]`".to_string()),
            None => return Err("array is not closed".to_string()),
            Some(_) => {}
        }
        values.push(parser.parse_string()?);
        parser.skip_whitespace();
        match parser.peek() {
            Some(',') => {
                parser.position += 1;
                after_comma = true;
            }
            Some(']') => break,
            Some(other) => {
                return Err(format!(
                    "expected `,` or `]` at character {}, found `{other}`",
                    parser.position
                ))
            }
            None => return Err("array is not closed".to_string()),
        }
    }

    // Exactly one `]` closes the array; anything after it is trailing content.
    parser.expect(']')?;

    parser.skip_whitespace();
    if parser.position != parser.chars.len() {
        return Err(format!(
            "trailing content after the array at character {}",
            parser.position
        ));
    }
    Ok(values)
}

/// A minimal, strict reader for the one JSON shape this crate persists.
struct JsonArrayParser<'a> {
    chars: &'a [char],
    position: usize,
}

impl JsonArrayParser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.position += 1;
        }
    }

    fn expect(&mut self, expected: char) -> std::result::Result<(), String> {
        match self.peek() {
            Some(found) if found == expected => {
                self.position += 1;
                Ok(())
            }
            Some(found) => Err(format!(
                "expected `{expected}` at character {}, found `{found}`",
                self.position
            )),
            None => Err(format!("expected `{expected}` but the value ended")),
        }
    }

    fn parse_string(&mut self) -> std::result::Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();

        loop {
            let character = match self.peek() {
                Some(character) => character,
                None => return Err("string is not terminated".to_string()),
            };
            self.position += 1;

            match character {
                '"' => return Ok(out),
                '\\' => out.push(self.parse_escape()?),
                // A raw control character inside a string is illegal JSON, so it is refused rather than
                // accepted as if it had been escaped.
                c if (c as u32) < 0x20 => {
                    return Err(format!(
                        "unescaped control character U+{:04X} in a string",
                        c as u32
                    ))
                }
                c => out.push(c),
            }
        }
    }

    fn parse_escape(&mut self) -> std::result::Result<char, String> {
        let escape = match self.peek() {
            Some(escape) => escape,
            None => return Err("escape sequence is incomplete".to_string()),
        };
        self.position += 1;

        match escape {
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '/' => Ok('/'),
            'b' => Ok('\u{08}'),
            'f' => Ok('\u{0c}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.parse_unicode_escape(),
            other => Err(format!("`\\{other}` is not a legal escape")),
        }
    }

    fn parse_unicode_escape(&mut self) -> std::result::Result<char, String> {
        let value = self.parse_hex4()?;

        // A high surrogate must be followed by its low surrogate; each half alone is not a character.
        if (0xD800..0xDC00).contains(&value) {
            self.expect('\\')?;
            self.expect('u')?;
            let low = self.parse_hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(format!(
                    "\\u{value:04X} is followed by a value that is not a low surrogate"
                ));
            }
            let combined = 0x1_0000_u32
                .checked_add((value - 0xD800) << 10)
                .and_then(|base| base.checked_add(low - 0xDC00))
                .ok_or_else(|| "surrogate pair is out of range".to_string())?;
            return char::from_u32(combined)
                .ok_or_else(|| "surrogate pair is not a character".to_string());
        }

        char::from_u32(value).ok_or_else(|| format!("\\u{value:04X} is not a character"))
    }

    fn parse_hex4(&mut self) -> std::result::Result<u32, String> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let digit = match self.peek() {
                Some(digit) => digit,
                None => return Err("\\u escape is truncated".to_string()),
            };
            self.position += 1;
            let nibble = digit
                .to_digit(16)
                .ok_or_else(|| format!("`{digit}` is not a hex digit"))?;
            value = value * 16 + nibble;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_array_round_trips_through_its_json_form() {
        let values = vec![
            "plain".to_string(),
            "quoted \"inner\"".to_string(),
            "back\\slash".to_string(),
            "line\nbreak\ttab".to_string(),
            "control \u{1}".to_string(),
            "unicode \u{1F600} and accent é".to_string(),
        ];
        let encoded = encode_json_string_array(&values);
        assert!(encoded.starts_with('['));
        assert_eq!(decode_json_string_array(&encoded), Ok(values.clone()));
        assert_eq!(
            decode_json_string_array(&encode_json_string_array(&[])),
            Ok(Vec::new())
        );
    }

    #[test]
    fn the_decoder_accepts_the_whitespace_and_escapes_a_conforming_encoder_may_emit() {
        assert_eq!(
            decode_json_string_array(" [ \"a\" ,\n\t\"b\" ] "),
            Ok(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            decode_json_string_array(r#"["\u0041\u00e9"]"#),
            Ok(vec!["Aé".to_string()])
        );
        assert_eq!(
            decode_json_string_array(r#"["\ud83d\ude00"]"#),
            Ok(vec!["\u{1F600}".to_string()])
        );
        assert_eq!(
            decode_json_string_array(r#"["a\/b"]"#),
            Ok(vec!["a/b".to_string()])
        );
        assert_eq!(
            decode_json_string_array(r#"["\b\f"]"#),
            Ok(vec!["\u{08}\u{0c}".to_string()])
        );
    }

    #[test]
    fn the_decoder_refuses_anything_it_cannot_read_exactly() {
        for malformed in [
            "",
            "null",
            "\"a\"",
            "[",
            "[,]",
            "[1]",
            "[\"a\",]",
            "[ \"a\"",
            "[\"a\"] trailing",
            "[\"unterminated]",
            "[\"raw\nnewline\"]",
            "[\"\\q\"]",
            "[\"\\u12\"]",
            "[\"\\u12zz\"]",
            "[\"\\ud83d\"]",
            "[\"\\ud83dx\"]",
        ] {
            assert!(
                decode_json_string_array(malformed).is_err(),
                "`{malformed}` must not decode"
            );
        }
    }

    #[test]
    fn the_mode_inputs_object_matches_the_contract_shape() {
        let radius = CouncilBlastRadius {
            scope_roots: vec!["crates/council".to_string(), "crates/storage".to_string()],
            affected_file_count: 12,
            crosses_workspace_boundary: true,
        };
        let json = mode_inputs_json(&radius, 2, 1);
        assert_eq!(
            json,
            r#"{"blast_radius":{"scope_roots":["crates/council","crates/storage"],"affected_file_count":12,"crosses_workspace_boundary":true},"prior_validation_failures":2,"open_disputes":1}"#
        );
        for key in [
            "blast_radius",
            "scope_roots",
            "affected_file_count",
            "prior_validation_failures",
            "open_disputes",
        ] {
            assert!(json.contains(key), "the inputs object must carry `{key}`");
        }
    }

    #[test]
    fn the_claim_grade_basis_object_records_both_outcomes() {
        let basis = CouncilClaimGradeBasis {
            claim_id: "claim_1".to_string(),
            reason: "WEAKEST_LOAD_BEARING_CLAIM".to_string(),
            resolved: vec![CouncilClaimReference {
                reference: "evidence:ev_1".to_string(),
                reason: "RESOLVED".to_string(),
            }],
            unresolved: vec![CouncilClaimReference {
                reference: "doc:missing.md".to_string(),
                reason: "REFERENCE_NOT_FOUND".to_string(),
            }],
        };
        let json = claim_grade_basis_json(&basis);
        assert!(json.contains("\"claim_id\":\"claim_1\""));
        assert!(json.contains("\"resolved\":[{\"reference\":\"evidence:ev_1\""));
        assert!(json.contains("\"unresolved\":[{\"reference\":\"doc:missing.md\""));
        assert!(json.contains("REFERENCE_NOT_FOUND"));
    }

    #[test]
    fn a_value_outside_a_closed_vocabulary_is_refused_with_the_column_named() {
        assert_eq!(
            require_vocabulary("role", "SKEPTIC", ROLE_VOCABULARY).ok(),
            Some("SKEPTIC")
        );
        match require_vocabulary("role", "AUDITOR", ROLE_VOCABULARY) {
            Err(StorageError::Malformed { column, detail }) => {
                assert_eq!(column, "role");
                assert!(detail.contains("AUDITOR"));
                assert!(
                    detail.contains("SKEPTIC"),
                    "the refusal must name the legal values"
                );
            }
            other => assert!(other.is_ok(), "expected a malformed-value refusal"),
        }
    }

    #[test]
    fn the_declared_vocabularies_match_the_schema_check_lists() {
        assert_eq!(DECISION_CLASS_VOCABULARY.len(), 6);
        assert_eq!(MODE_VOCABULARY, &["SOLO", "REVIEW", "FULL"]);
        assert_eq!(OVERRIDE_SOURCE_VOCABULARY, &["NONE", "USER"]);
        assert_eq!(ROLE_VOCABULARY, &["PROPOSER", "SKEPTIC", "VERIFIER"]);
        assert_eq!(CLAIM_GRADE_VOCABULARY, &["ASSUMPTION", "CITED", "VERIFIED"]);
        assert_eq!(BUDGET_KIND_VOCABULARY.len(), 7);
        assert_eq!(BUDGET_AVAILABILITY_VOCABULARY, &["REPORTED", "UNAVAILABLE"]);
        assert_eq!(
            OUTCOME_STATUS_VOCABULARY,
            &["HELD", "AMENDED", "REVERSED", "UNRESOLVED"]
        );
        assert_eq!(
            OUTCOME_SOURCE_VOCABULARY,
            &["VALIDATION_RESULT", "REOPEN_DECISION", "USER_SUPERSESSION"]
        );
    }
}
