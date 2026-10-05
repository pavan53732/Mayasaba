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

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::chain::{ChainEvent, ChainScope, ChainVerification, EventHashFields, GENESIS_PREV_HASH};

/// RFC 8785 JCS canonicalization, shared by the DEC-025 state digest and the DEC-034 event chain.
pub mod canonical;
/// The DEC-034 per-project event hash chain.
pub mod chain;

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
    /// An event names neither a project nor a session, so it belongs to no hash chain (DEC-034).
    ///
    /// Refused rather than stored unchained: an event outside every chain is an event whose alteration
    /// nothing can detect, so accepting one would leave a class of durable history outside the immutability
    /// rule while `verify_event_chain` still reported success.
    UnscopedEvent,
    /// A value could not be reduced to its RFC 8785 canonical form, so no digest over it is well defined.
    Canonical(crate::canonical::CanonicalError),
    /// A message is not in a terminal state, so there is nothing to replay.
    ///
    /// Only `EXPIRED` and `DEAD_LETTER` end a message's life. A message still in flight has not finished failing,
    /// so replaying it would duplicate work that is still in progress - and the declared machine has no edge from
    /// any in-flight state back to `CREATED`, so there would be no way to express it even if it were wanted.
    NotTerminal {
        message_id: String,
        delivery_state: String,
    },
    /// A second message already occupies the same `(session_id, channel, sequence)`.
    ///
    /// The table's `UNIQUE` constraint is the enforcement; this variant is the *named* report of it, so a
    /// caller is not handed a raw SQLite constraint string and forced to parse it to learn what happened.
    SequenceConflict {
        session_id: String,
        channel: String,
        sequence: i64,
    },
    /// A task attempt presents a lease fence that is no longer authoritative.
    StaleFence { attempt_id: String, attempt_fence: i64, current_lease_version: i64, presented: i64 },
    /// A task-attempt state change is not allowed by the attempt lifecycle.
    InvalidAttemptTransition { attempt_id: String, from: String, to: String },

    /// A write named a project that does not exist.
    ///
    /// `messages`, `outbox`, `message_receipts` and `dead_letters` each carry a foreign key to `projects`, so
    /// the write would fail regardless. Reporting it here names the missing project; SQLite's own
    /// "FOREIGN KEY constraint failed" does not say which reference was dangling, and a caller forced to parse
    /// that string to learn what happened would be reading a message rather than an error.
    UnknownProject { project_id: String },
    /// A column whose name ends in `_json` was handed text that is not JSON.
    ///
    /// The canonical schema puts no `CHECK (json_valid(...))` on any JSON column, so this is not a constraint
    /// the schema declares. It is a precondition of a behaviour the contract does declare: the idempotency
    /// scope is `project_id + operation_id` and `operation_id` has no column, so finding it means calling
    /// SQLite's `json_extract` on `envelope_json`, and that function raises on malformed input. Without this
    /// check the same malformed text would be refused when the message carried an `operation_id` and accepted
    /// when it did not - the same invalid input behaving differently because of an unrelated field. Refusing it
    /// once, by name, removes that.
    MalformedJson { column: String },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Schema(e) => write!(f, "canonical schema could not be applied: {e}"),
            StorageError::Db(e) => write!(f, "storage operation failed: {e}"),
            StorageError::StaleFence { attempt_id, attempt_fence, current_lease_version, presented } => write!(f, "stale task-attempt fence: attempt={attempt_id}, attempt_fence={attempt_fence}, current_lease_version={current_lease_version}, presented={presented}"),
            StorageError::InvalidAttemptTransition { attempt_id, from, to } => write!(f, "invalid task-attempt transition: {attempt_id}: {from} -> {to}"),
            StorageError::NotFound(what) => write!(f, "not found: {what}"),
            StorageError::NotTerminal {
                message_id,
                delivery_state,
            } => write!(
                f,
                "message {message_id} is {delivery_state}, which is not terminal, so it cannot be replayed"
            ),
            StorageError::Malformed { column, detail } => {
                write!(
                    f,
                    "stored value in `{column}` cannot be read back: {detail}"
                )
            }
            StorageError::UnscopedEvent => write!(
                f,
                "event names neither a project_id nor a session_id, so it belongs to no hash chain"
            ),
            StorageError::Canonical(e) => write!(f, "{e}"),
            StorageError::SequenceConflict {
                session_id,
                channel,
                sequence,
            } => write!(
                f,
                "sequence {sequence} on channel `{channel}` for session `{session_id}` is already taken; \
                 (session_id, channel, sequence) is unique"
            ),
            StorageError::UnknownProject { project_id } => {
                write!(f, "no project `{project_id}` exists to write for")
            }
            StorageError::MalformedJson { column } => {
                write!(f, "`{column}` must hold JSON text, and this is not JSON")
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

/// An `events` row to append, with its chain link computed here rather than supplied.
///
/// `prev_hash`, `event_hash` and `sequence` are deliberately absent: they are derived, not authored. A caller
/// that could pass a hash could persist a link that does not belong to its chain, which is exactly the
/// corruption DEC-034 exists to detect - and it would be indistinguishable from a genuine edit afterwards.
///
/// `sequence` is derived for the same reason and one more. DEC-034 orders a chain by `sequence`, and the column
/// is nullable with no uniqueness on it, so a caller-supplied position could collide or leave a hole and the
/// chain would still verify. Deriving it as one past the chain's last positioned event makes the order a strict
/// total order by construction. It is not the envelope's `sequence`: that one is the sender's ordering position
/// within a `(session_id, channel)`, is stored on `messages`, and is the sender's to declare. The two columns
/// share a name and nothing else, which is worth stating because they are easy to conflate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEvent {
    pub event_id: String,
    /// The project whose chain this event extends. `None` chains by `session_id` instead.
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub event_type: String,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub epoch: Option<i64>,
    /// JSON text, hashed verbatim. See `chain::EventHashFields::payload_json` for why it is not re-encoded.
    pub payload_json: String,
    pub created_at: String,
}

/// An outbound message to persist, queue and record, in one transaction.
///
/// The envelope is supplied as text, not as a parsed value, because that text is what gets stored: hashing,
/// comparing a retry against its original, or re-encoding it would all be defeated by a second
/// serialization of the same message. The caller validates it before calling; this crate stores what it is
/// given.
///
/// `message_id`, `event_id`, `project_id`, `session_id`, `channel`, `sequence` and `correlation_id` are all
/// non-null because the MCF-v2 contract requires them on every envelope, so a message that reached here
/// without one is not a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOutboundMessage {
    /// The envelope's own `message_id`.
    pub message_id: String,
    /// The envelope's own `event_id`, which the sender declared as the event this message carries.
    ///
    /// Distinct from the `event_id` of the lifecycle event rows the bus appends for this message: those are
    /// this crate's record of the delivery transitions, and this is the sender's statement about its own event.
    pub event_id: String,
    pub project_id: String,
    pub session_id: String,
    pub message_type: String,
    pub channel: String,
    /// The sender's ordering position within `(session_id, channel)`.
    pub sequence: i64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub idempotency_key: Option<String>,
    /// The operation this message performs. With `project_id` this is the canonical idempotency scope
    /// (DEC-027 and the implementation design's "Idempotency scope").
    pub operation_id: Option<String>,
    pub envelope_json: String,
    pub created_at: String,
    pub project_epoch: i64,
}

/// The facts a replay decision needs, read without writing anything.
///
/// `Bus::replay` rewrites the envelope as it copies it, so a caller that has to decide *whether* to replay
/// cannot decide by calling it - by the time it had an answer the replay would have happened. These three facts
/// are read in one query for that reason, and `message_type` is read from its own column rather than parsed out
/// of the envelope, because the column is the stored projection the insert already made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFacts {
    /// The envelope's `message_type`, as stored.
    pub message_type: String,
    /// The context the message was decided in, absent when it is not a material action.
    pub context_snapshot_id: Option<String>,
    /// The state digest the message was decided against, absent when it is not a material action.
    pub state_digest: Option<String>,
}

/// A terminal message rewritten as a new one, ready to be enqueued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaySource {
    /// The original envelope with `message_id`, `sequence`, `created_at` and `causation_id` rewritten.
    pub envelope_json: String,
    /// The original's project, which the replay belongs to as well.
    pub project_id: String,
    /// The ordering position the replay took.
    pub sequence: i64,
    /// The original's terminal event, which the replay names as its cause.
    pub terminal_event_id: Option<String>,
}

/// What an enqueue did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueuedMessage {
    pub message_id: String,
    pub outbox_id: String,
    /// True when an earlier enqueue of the same message or the same `(project_id, operation_id)` was found.
    ///
    /// Reported rather than hidden, because "this was already queued" and "this is now queued" are different
    /// answers to the caller's request, and a caller that cannot tell them apart cannot tell a retry that was
    /// absorbed from a retry that created a second delivery.
    pub deduplicated: bool,
    /// The stored envelope of the message this enqueue matched, when it matched one.
    ///
    /// Returned so the caller can compare it with what it just tried to enqueue. A retry that reuses an
    /// idempotency identity with a different body must not be absorbed as a duplicate: the registry's
    /// `IDEMPOTENCY_CONFLICT` says the two cannot both be applied, and only the caller holds both documents.
    pub stored_envelope_json: Option<String>,
    /// The stored envelope's `payload` member, extracted by SQLite, when this enqueue matched one.
    ///
    /// Separate from `stored_envelope_json` because comparing whole envelopes answers the wrong question. A
    /// retry legitimately carries a new `message_id`, `sequence` and `created_at`, so whole-envelope equality
    /// would report every retry as a conflict. The payload is the request body, and it is what must match.
    ///
    /// Extracted here rather than parsed by the caller because this crate is already talking to a JSON engine
    /// and the caller is not: `json_extract` also normalizes the two sides, so `{"a":1,"b":2}` and
    /// `{"b":2,"a":1}` compare equal, which is what "the same body" means.
    pub stored_payload_json: Option<String>,
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

/// Append `new` to its chain inside `tx`, computing the link from the chain's current tail.
///
/// A free function rather than a method because it takes the caller's transaction. `create_project` and the
/// bus's outbox write both need the event to commit with their own state change, and neither can borrow the
/// `Connection` again while its transaction is open.
fn append_event_in(tx: &rusqlite::Transaction<'_>, new: &NewEvent) -> Result<()> {
    let scope = ChainScope::of(new.project_id.as_deref(), new.session_id.as_deref())?;
    let (last_sequence, prev_hash) = chain_tail(tx, &scope)?;
    // One past the last positioned event. Reading the tail's sequence and its hash in the same query is not an
    // optimisation: a position computed from a different read than the link would let the two disagree.
    let sequence = last_sequence + 1;
    let event_hash = chain::event_hash(&EventHashFields {
        prev_hash: &prev_hash,
        event_id: &new.event_id,
        project_id: new.project_id.as_deref(),
        session_id: new.session_id.as_deref(),
        event_type: &new.event_type,
        sequence: Some(sequence),
        correlation_id: new.correlation_id.as_deref(),
        causation_id: new.causation_id.as_deref(),
        epoch: new.epoch,
        payload_json: &new.payload_json,
        created_at: &new.created_at,
    })?;

    tx.execute(
        "INSERT INTO events (event_id, project_id, session_id, event_type, sequence, correlation_id,
                             causation_id, epoch, payload_json, prev_hash, event_hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            new.event_id,
            new.project_id,
            new.session_id,
            new.event_type,
            sequence,
            new.correlation_id,
            new.causation_id,
            new.epoch,
            new.payload_json,
            prev_hash,
            event_hash,
            new.created_at,
        ],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// The last positioned link in `scope`'s chain: its `sequence` and its `event_hash`.
///
/// An empty chain reports sequence 0 and the genesis hash, so the first appended event is sequence 1 - the same
/// position `create_project` gives a project's genesis event.
///
/// `sequence IS NOT NULL` matches the walk in `chain::verify`: an event with no sequence has no position in an
/// ordered chain, so it cannot be a predecessor. Ordering by `sequence` with `rowid` as the tiebreaker is the
/// same order DEC-034 defines, so the link this returns is the one verification will expect.
fn chain_tail(tx: &rusqlite::Transaction<'_>, scope: &ChainScope) -> Result<(i64, String)> {
    let sql = match scope {
        ChainScope::Project(_) => {
            "SELECT sequence, event_hash FROM events WHERE project_id = ?1 AND sequence IS NOT NULL
             ORDER BY sequence DESC, rowid DESC LIMIT 1"
        }
        // `project_id IS NULL` is not redundant with the scope: without it, a session chain would inherit the
        // tail of a project chain whenever the two identifiers happened to be spelled the same.
        ChainScope::Session(_) => {
            "SELECT sequence, event_hash FROM events WHERE project_id IS NULL AND session_id = ?1
             AND sequence IS NOT NULL ORDER BY sequence DESC, rowid DESC LIMIT 1"
        }
    };
    let tail: Option<(i64, String)> = tx
        .query_row(sql, rusqlite::params![scope.value()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(StorageError::Db)?;
    Ok(tail.unwrap_or_else(|| (0, GENESIS_PREV_HASH.to_string())))
}

/// The payload of a project's genesis event.
///
/// Canonicalized rather than interpolated, and that is a defect fix rather than a style preference: `local_path`
/// is a Windows path, so it contains backslashes, and a backslash interpolated into a JSON string is an invalid
/// escape. Every project this crate created before this change stored a `payload_json` that no JSON parser would
/// accept - and because DEC-034 hashes that text, the malformed document was on its way into the event chain as
/// a permanent, unverifiable-against-anything-else link.
fn project_created_payload(new: &NewProject) -> Result<String> {
    Ok(canonical::jcs_object(&[
        ("brief_id", canonical::JcsValue::Str(&new.brief_id)),
        ("epoch", canonical::JcsValue::Int(0)),
        ("local_path", canonical::JcsValue::Str(&new.local_path)),
    ])?)
}

/// The `outbox` row for a message.
///
/// `outbox.message_id` is `UNIQUE`, so the relation is one to one and the id is derived from the message rather
/// than minted. A second identifier source would be a second thing that can disagree with the relation the
/// schema already declares, and there would be nothing to reconcile it against.
fn outbox_id_for(message_id: &str) -> String {
    format!("obx_{message_id}")
}

/// The `events.event_id` of the lifecycle event a delivery transition records.
///
/// Derived from the message for the same reason the outbox id is: the pair `(message, transition)` is the
/// event's identity, and deriving it makes that identity checkable by reading the id.
fn transition_event_id(message_id: &str, event_type: &str) -> String {
    format!("evt_{message_id}_{}", event_type.to_ascii_lowercase())
}

/// The payload of a delivery-transition event.
///
/// Built with the canonicalizer rather than with `format!`, because a JSON document assembled by string
/// interpolation stops being JSON the moment a value contains a quote or a backslash - and a message id is
/// only `format`-annotated in the contract, which JSON Schema treats as annotation rather than validation. The
/// payload is also one of the fields DEC-034 hashes, so an unescaped value would be baked into the chain.
fn transition_payload(new: &NewOutboundMessage) -> Result<String> {
    Ok(canonical::jcs_object(&[
        ("channel", canonical::JcsValue::Str(&new.channel)),
        ("message_id", canonical::JcsValue::Str(&new.message_id)),
        ("message_type", canonical::JcsValue::Str(&new.message_type)),
        ("sequence", canonical::JcsValue::Int(new.sequence)),
    ])?)
}

/// Persist, queue and record an outbound message inside `tx`.
///
/// The order of the checks is the order of the identity they test, strongest first: an enqueue that matches an
/// existing message is a duplicate of that message whatever else is true; failing that, one that matches a
/// project operation is a retry of that operation; failing both, the position it claims must be free.
fn enqueue_message_in(
    tx: &rusqlite::Transaction<'_>,
    new: &NewOutboundMessage,
) -> Result<EnqueuedMessage> {
    if !project_exists(tx, &new.project_id)? {
        return Err(StorageError::UnknownProject {
            project_id: new.project_id.clone(),
        });
    }
    if !is_json(tx, &new.envelope_json)? {
        return Err(StorageError::MalformedJson {
            column: "messages.envelope_json".to_string(),
        });
    }

    if let Some(existing) = find_enqueued_message(tx, &new.message_id)? {
        return Ok(EnqueuedMessage {
            deduplicated: true,
            ..existing
        });
    }
    if let Some(operation_id) = new.operation_id.as_deref() {
        if let Some(existing) = find_outbound_by_operation_in(tx, &new.project_id, operation_id)? {
            return Ok(EnqueuedMessage {
                deduplicated: true,
                ..existing
            });
        }
    }

    // Refused before the insert rather than after it, so the caller gets a sentence naming the position instead
    // of a raw `UNIQUE constraint failed: messages.session_id, messages.channel, messages.sequence`. The
    // constraint remains the enforcement; this is the report.
    if find_message_at_position(tx, &new.session_id, &new.channel, new.sequence)?.is_some() {
        return Err(StorageError::SequenceConflict {
            session_id: new.session_id.clone(),
            channel: new.channel.clone(),
            sequence: new.sequence,
        });
    }

    // CREATED -> PERSISTED. The row is inserted already carrying the state this transition produced, so a row
    // can never be observed in CREATED: that state describes a message that exists only in memory.
    tx.execute(
        "INSERT OR IGNORE INTO messages (message_id, event_id, project_id, session_id, message_type, channel, sequence,
                               correlation_id, causation_id, idempotency_key, delivery_state, envelope_json,
                               created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'PERSISTED', ?11, ?12)",
        rusqlite::params![
            new.message_id,
            new.event_id,
            new.project_id,
            new.session_id,
            new.message_type,
            new.channel,
            new.sequence,
            new.correlation_id,
            new.causation_id,
            new.idempotency_key,
            new.envelope_json,
            new.created_at,
        ],
    )
    .map_err(StorageError::Db)?;

    let payload = transition_payload(new)?;
    append_event_in(
        tx,
        &NewEvent {
            event_id: transition_event_id(&new.message_id, "MESSAGE_PERSISTED"),
            project_id: Some(new.project_id.clone()),
            session_id: Some(new.session_id.clone()),
            event_type: "MESSAGE_PERSISTED".to_string(),
            correlation_id: Some(new.correlation_id.clone()),
            // The sender's declared cause, propagated rather than replaced: both lifecycle events of one
            // enqueue have the same upstream cause, because they are two steps of one act (AGENTS.md section 10).
            causation_id: new.causation_id.clone(),
            epoch: Some(new.project_epoch),
            payload_json: payload.clone(),
            created_at: new.created_at.clone(),
        },
    )?;

    // PERSISTED -> QUEUED. The outbox row is what makes the message dispatchable, and `next_attempt_at` starts
    // at the queueing time so "is it due?" is one comparison for a first attempt and a retry alike.
    //
    // The stamp is normalized through SQLite rather than copied. `created_at` comes from the envelope and the
    // contract types it as `format: date-time`, which admits an offset (`2026-10-04T02:00:05+02:00`); the due
    // check compares this column with a UTC clock stamp, and that comparison is only correct while every value
    // in the column is the same fixed-width UTC spelling. `messages.created_at` keeps the sender's own spelling,
    // because that column is the record of what the sender said.
    let outbox_id = outbox_id_for(&new.message_id);
    tx.execute(
        "INSERT INTO outbox (outbox_id, message_id, project_id, queued_at, dispatch_state, next_attempt_at,
                             attempts)
         VALUES (?1, ?2, ?3, ?4, 'PENDING', strftime('%Y-%m-%dT%H:%M:%SZ', datetime(?4)), 0)",
        rusqlite::params![
            outbox_id,
            new.message_id,
            new.project_id,
            new.created_at,
        ],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE messages SET delivery_state = 'QUEUED' WHERE message_id = ?1",
        rusqlite::params![new.message_id],
    )
    .map_err(StorageError::Db)?;
    append_event_in(
        tx,
        &NewEvent {
            event_id: transition_event_id(&new.message_id, "MESSAGE_QUEUED"),
            project_id: Some(new.project_id.clone()),
            session_id: Some(new.session_id.clone()),
            event_type: "MESSAGE_QUEUED".to_string(),
            correlation_id: Some(new.correlation_id.clone()),
            causation_id: new.causation_id.clone(),
            epoch: Some(new.project_epoch),
            payload_json: payload,
            created_at: new.created_at.clone(),
        },
    )?;

    Ok(EnqueuedMessage {
        message_id: new.message_id.clone(),
        outbox_id,
        deduplicated: false,
        stored_envelope_json: None,
        stored_payload_json: None,
    })
}

fn project_exists(conn: &Connection, project_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM projects WHERE project_id = ?1",
        rusqlite::params![project_id],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(StorageError::Db)
}

/// Whether `text` is JSON, decided by SQLite's own parser.
///
/// `json_valid` rather than a Rust parser because this crate has no JSON dependency and should not gain one to
/// answer a question the database it is already talking to can answer exactly.
fn is_json(conn: &Connection, text: &str) -> Result<bool> {
    conn.query_row("SELECT json_valid(?1)", rusqlite::params![text], |r| {
        r.get(0)
    })
    .map_err(StorageError::Db)
}

/// The queue entry for a message, if it was enqueued.
///
/// A `messages` row with no `outbox` row is reported rather than treated as either a duplicate or a new
/// message. Nothing this crate writes can produce one - both rows go in together - so its presence means the
/// table was written outside this crate, and guessing which of the two it is would be guessing about history.
fn find_enqueued_message(conn: &Connection, message_id: &str) -> Result<Option<EnqueuedMessage>> {
    let row: Option<(Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT o.outbox_id, m.envelope_json, json_extract(m.envelope_json, '$.payload')
             FROM messages m LEFT JOIN outbox o ON o.message_id = m.message_id
             WHERE m.message_id = ?1",
            rusqlite::params![message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(StorageError::Db)?;

    match row {
        None => Ok(None),
        Some((Some(outbox_id), envelope_json, payload_json)) => Ok(Some(EnqueuedMessage {
            message_id: message_id.to_string(),
            outbox_id,
            deduplicated: false,
            stored_envelope_json: Some(envelope_json),
            stored_payload_json: payload_json,
        })),
        Some((None, _, _)) => Err(StorageError::Malformed {
            column: "outbox.message_id".to_string(),
            detail: format!(
                "message `{message_id}` exists with no outbox row, so it was never queued; re-queueing it \
                 would rewrite the record of what happened"
            ),
        }),
    }
}

/// The existing message that already claims a `(project_id, operation_id)`, if any is still live.
///
/// **Only a live claim counts.** Idempotency exists to stop one operation being queued twice while it is still
/// pending; once it has terminated, re-enqueueing that operation is exactly what a replay is, so a terminal
/// message must not absorb the replay as a duplicate of the very message being replayed. This was found by
/// replay: the replay carries the original's `operation_id` - it is the same operation - so without this the
/// replay was refused as a duplicate of its own original and `Bus::replay` returned the original's identity.
///
/// `PROCESSED` is deliberately not excluded. It is terminal, but it is a receiver's outcome rather than a
/// sender's, and an outbound message reaching it would mean the sender processed its own message - a case this
/// crate does not produce, so excluding it would be a rule with no case behind it.
fn find_outbound_by_operation_in(
    conn: &Connection,
    project_id: &str,
    operation_id: &str,
) -> Result<Option<EnqueuedMessage>> {
    conn.query_row(
        "SELECT m.message_id, o.outbox_id, m.envelope_json, json_extract(m.envelope_json, '$.payload')
         FROM messages m JOIN outbox o ON o.message_id = m.message_id
         WHERE m.project_id = ?1 AND json_extract(m.envelope_json, '$.operation_id') = ?2
           AND m.delivery_state NOT IN ('EXPIRED', 'DEAD_LETTER')
         ORDER BY m.rowid LIMIT 1",
        rusqlite::params![project_id, operation_id],
        |r| {
            Ok(EnqueuedMessage {
                message_id: r.get(0)?,
                outbox_id: r.get(1)?,
                deduplicated: false,
                stored_envelope_json: Some(r.get(2)?),
                stored_payload_json: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(StorageError::Db)
}

fn find_message_at_position(
    conn: &Connection,
    session_id: &str,
    channel: &str,
    sequence: i64,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT message_id FROM messages WHERE session_id = ?1 AND channel = ?2 AND sequence = ?3",
        rusqlite::params![session_id, channel, sequence],
        |r| r.get(0),
    )
    .optional()
    .map_err(StorageError::Db)
}

/// One queue entry that is due to be dispatched.
///
/// Carries the stored envelope rather than a parsed one, because what a transport sends must be what was
/// stored: a dispatcher that re-serialized the message could send something the durable record does not
/// describe.
/// A position missing between two observed positions in one `(session_id, channel)` stream.
///
/// Derived from `messages` rather than stored: the positions are already durable there, and a second record of
/// the same fact is a second fact that can disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceGapRow {
    pub session_id: String,
    pub channel: String,
    /// The absent position.
    pub expected: i64,
    /// The observed position that revealed it: the next position actually present after the hole.
    pub found: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueMessage {
    pub message_id: String,
    pub project_id: String,
    pub session_id: String,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub message_type: String,
    pub channel: String,
    pub sequence: i64,
    pub envelope_json: String,
    /// Attempts recorded so far, from `outbox.attempts`. The dispatch decision is taken from this, not from a
    /// counter in memory, so a restart cannot reset a message's budget.
    pub attempts: i64,
    pub next_attempt_at: String,
}

/// The outcome of one delivery attempt, as the dispatcher observed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryAttempt {
    /// The attempt number this was, counting from 1.
    pub attempt_no: i64,
    pub started_at: String,
    pub finished_at: String,
    /// A short machine-readable outcome for `message_attempts.outcome`.
    pub outcome: String,
    /// The registry code of the failure, or `None` when the attempt succeeded.
    pub error_code: Option<String>,
    /// Human-readable detail for the failure, or `None` when the attempt succeeded.
    pub error_detail: Option<String>,
}

/// What the dispatcher is allowed to claim next.
///
/// A due entry is `dispatch_state = 'PENDING'` and `next_attempt_at` at or before `now`. Ordering is by
/// `next_attempt_at` then `rowid`, so the oldest due entry goes first and ties are broken by insertion order
/// rather than by whatever order the planner happens to return.
fn due_outbound_in(conn: &Connection, now: &str, limit: i64) -> Result<Vec<DueMessage>> {
    let mut stmt = conn
        .prepare(
            "SELECT m.message_id, m.project_id, m.session_id, m.correlation_id, m.causation_id,
                    m.message_type, m.channel, m.sequence, m.envelope_json, o.attempts, o.next_attempt_at
             FROM outbox o JOIN messages m ON m.message_id = o.message_id
             WHERE o.dispatch_state IN ('PENDING', 'FAILED')
               AND o.next_attempt_at <= ?1
               AND m.delivery_state = 'QUEUED'
             ORDER BY CASE json_extract(m.envelope_json, '$.priority')
                        WHEN 'EMERGENCY_CONTROL' THEN 0
                        WHEN 'SYNCHRONIZATION' THEN 1
                        WHEN 'TASK_CONTROL' THEN 2
                        WHEN 'FAILURE_RECOVERY' THEN 3
                        WHEN 'COUNCIL' THEN 4
                        WHEN 'PROGRESS_HEARTBEAT' THEN 5
                        WHEN 'BULK' THEN 6
                        ELSE 7
                      END,
                      o.next_attempt_at,
                      o.rowid
             LIMIT ?2",
        )
        .map_err(StorageError::Db)?;
    let rows = stmt
        .query_map(rusqlite::params![now, limit], |r| {
            Ok(DueMessage {
                message_id: r.get(0)?,
                project_id: r.get(1)?,
                session_id: r.get(2)?,
                correlation_id: r.get(3)?,
                causation_id: r.get(4)?,
                message_type: r.get(5)?,
                channel: r.get(6)?,
                sequence: r.get(7)?,
                envelope_json: r.get(8)?,
                attempts: r.get(9)?,
                next_attempt_at: r.get(10)?,
            })
        })
        .map_err(StorageError::Db)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(StorageError::Db)
}

/// Record an attempt that did not hand the message over, and schedule the next one.
///
/// **This does not advance `messages.delivery_state`.** The `message_delivery` machine declares no transition
/// for a failed handover, and the accepted constraint is that transport failures never move the message: it
/// stays `QUEUED` until it either dispatches or expires. There is therefore also no event to append, because the
/// event log records declared transitions and this is not one; `message_attempts` is the durable record of what
/// was tried.
///
/// The next attempt time is computed by SQLite from the current one plus `backoff_seconds`, so the bus needs no
/// date arithmetic and the column keeps one spelling.
fn record_failed_attempt_in(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    attempt: &DeliveryAttempt,
    backoff_seconds: i64,
) -> Result<()> {
    insert_attempt(tx, message_id, attempt)?;
    tx.execute(
        "UPDATE outbox
         SET attempts = ?2,
             dispatch_state = 'FAILED',
             next_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ', datetime(?4, '+' || ?3 || ' seconds'))
         WHERE message_id = ?1",
        rusqlite::params![
            message_id,
            attempt.attempt_no,
            backoff_seconds,
            attempt.finished_at
        ],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// Record an attempt that handed the message over, and advance `QUEUED -> DISPATCHED`.
///
/// The transition is the declared one: event `MESSAGE_DISPATCHED`, command `ADVANCE_MESSAGE_DELIVERY`, state
/// mutation to `DISPATCHED`, outbox effect on the queue entry, epoch effect `NONE`.
///
/// It is a separate transaction from the handover, which is what `docs/MCF-V2-IMPLEMENTATION-DESIGN.md`
/// requires: "After send, delivery state is updated separately; a crash may cause duplicate delivery and must be
/// safe." A crash between the two leaves the message `QUEUED` and it is sent again, which is why the receiver
/// side has to be idempotent rather than why the sender should pretend the send did not happen.
fn record_dispatched_in(
    tx: &rusqlite::Transaction<'_>,
    due: &DueMessage,
    attempt: &DeliveryAttempt,
) -> Result<()> {
    insert_attempt(tx, &due.message_id, attempt)?;
    let event_id = next_transition_event_id(tx, &due.message_id, "MESSAGE_DISPATCHED")?;
    tx.execute(
        "UPDATE messages SET delivery_state = 'DISPATCHED' WHERE message_id = ?1",
        rusqlite::params![due.message_id],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE outbox SET attempts = ?2, dispatch_state = 'DISPATCHED', next_attempt_at = NULL
         WHERE message_id = ?1",
        rusqlite::params![due.message_id, attempt.attempt_no],
    )
    .map_err(StorageError::Db)?;
    append_event_in(
        tx,
        &NewEvent {
            event_id,
            project_id: Some(due.project_id.clone()),
            session_id: Some(due.session_id.clone()),
            event_type: "MESSAGE_DISPATCHED".to_string(),
            correlation_id: Some(due.correlation_id.clone()),
            causation_id: due.causation_id.clone(),
            // The transition record declares `epoch_effect: NONE` for every message_delivery edge, so the event
            // carries the epoch it was written under and does not change the project's.
            epoch: None,
            payload_json: transition_payload_fields(
                &due.message_id,
                &due.message_type,
                due.sequence,
            )?,
            created_at: attempt.finished_at.clone(),
        },
    )
}

/// Terminate a message whose attempt budget is exhausted, through the declared `QUEUED -> EXPIRED` edge.
///
/// The edge's event is `MESSAGE_EXPIRED` and its command is `EXPIRE_MESSAGE`. A `dead_letters` row records the
/// final error and the attempt count, because "dead-letter records retain original message identity, final
/// error, attempts and relevant causal references" and because the alternative - expiring silently - would lose
/// the reason.
///
/// This is deliberately **not** the `DEAD_LETTER` state. That state is reached by `PROCESSING -> REJECTED ->
/// DEAD_LETTER`, which is a receiver refusing a message it received; a sender that never managed to hand one
/// over has not been refused.
fn expire_message_in(
    tx: &rusqlite::Transaction<'_>,
    due: &DueMessage,
    finished_at: &str,
) -> Result<()> {
    let event_id = next_transition_event_id(tx, &due.message_id, "MESSAGE_EXPIRED")?;
    tx.execute(
        "UPDATE messages SET delivery_state = 'EXPIRED' WHERE message_id = ?1",
        rusqlite::params![due.message_id],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE outbox SET attempts = ?2, dispatch_state = 'ABANDONED', next_attempt_at = NULL
         WHERE message_id = ?1",
        rusqlite::params![due.message_id, due.attempts.max(0)],
    )
    .map_err(StorageError::Db)?;
    let final_error = final_error_json(tx, &due.message_id)?;
    tx.execute(
        "INSERT INTO dead_letters (dead_letter_id, message_id, project_id, final_error_json, attempts,
                                   created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            format!("dl_{}", due.message_id),
            due.message_id,
            due.project_id,
            final_error,
            due.attempts.max(0),
            finished_at,
        ],
    )
    .map_err(StorageError::Db)?;
    append_event_in(
        tx,
        &NewEvent {
            event_id,
            project_id: Some(due.project_id.clone()),
            session_id: Some(due.session_id.clone()),
            event_type: "MESSAGE_EXPIRED".to_string(),
            correlation_id: Some(due.correlation_id.clone()),
            causation_id: due.causation_id.clone(),
            epoch: None,
            payload_json: transition_payload_fields(
                &due.message_id,
                &due.message_type,
                due.sequence,
            )?,
            created_at: finished_at.to_string(),
        },
    )
}

fn insert_attempt(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    attempt: &DeliveryAttempt,
) -> Result<()> {
    tx.execute(
        "INSERT INTO message_attempts (attempt_id, message_id, attempt_no, started_at, finished_at, outcome,
                                       error_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            format!("att_{}_{}", message_id, attempt.attempt_no),
            message_id,
            attempt.attempt_no,
            attempt.started_at,
            attempt.finished_at,
            attempt.outcome,
            error_json(attempt)?,
        ],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// The attempt's failure as canonical JSON, or `NULL` when the attempt succeeded.
///
/// `message_attempts.error_json` is nullable, so a success stores no error rather than an empty object: "there
/// was no error" and "the error was nothing" are different statements and only the first is true here.
fn error_json(attempt: &DeliveryAttempt) -> Result<Option<String>> {
    match &attempt.error_code {
        None => Ok(None),
        Some(code) => Ok(Some(canonical::jcs_object(&[
            ("code", canonical::JcsValue::Str(code)),
            (
                "detail",
                canonical::JcsValue::Str(attempt.error_detail.as_deref().unwrap_or("")),
            ),
        ])?)),
    }
}

/// The final error of an expired message, as canonical JSON.
///
/// `dead_letters.final_error_json` is `NOT NULL`, so an expiry always has something to say. What it should say
/// is **why the message died**, and that is the last recorded failure, read back from `message_attempts`. A
/// message that expired with no recorded failure at all - which the dispatcher cannot produce, because expiry
/// follows a failed attempt - is recorded as an explicit absence rather than as an empty document.
///
/// The attempt count is not repeated inside the document because `dead_letters.attempts` is a column of its own,
/// and two copies of one fact are two facts that can disagree.
fn final_error_json(tx: &rusqlite::Transaction<'_>, message_id: &str) -> Result<String> {
    if let Some(recorded) = last_recorded_error(tx, message_id)? {
        return Ok(recorded);
    }
    Ok(canonical::jcs_object(&[
        ("code", canonical::JcsValue::Str("UNRECORDED")),
        ("detail", canonical::JcsValue::Str("")),
    ])?)
}

/// The most recent recorded failure for a message, exactly as it was stored.
fn last_recorded_error(tx: &rusqlite::Transaction<'_>, message_id: &str) -> Result<Option<String>> {
    tx.query_row(
        "SELECT error_json FROM message_attempts
         WHERE message_id = ?1 AND error_json IS NOT NULL
         ORDER BY attempt_no DESC LIMIT 1",
        rusqlite::params![message_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(StorageError::Db)
}

/// The payload of a dispatch-lifecycle event.
///
/// The transition records declare `required_fields: ["project_id", "correlation_id"]`, and both are carried by
/// the envelope, the chain scope and the `events` row rather than duplicated into the payload. What the payload
/// names is what the event is about: the message, what kind it was, and the position it claimed.
fn transition_payload_fields(
    message_id: &str,
    message_type: &str,
    sequence: i64,
) -> Result<String> {
    Ok(canonical::jcs_object(&[
        ("message_id", canonical::JcsValue::Str(message_id)),
        ("message_type", canonical::JcsValue::Str(message_type)),
        ("sequence", canonical::JcsValue::Int(sequence)),
    ])?)
}

/// The same payload with extra members. The caller may pass them in any order, because `jcs_object` sorts them:
/// a payload stays canonical however it was assembled, which is what DEC-034 needs of every hashed document.
fn transition_payload_fields_and(
    message_id: &str,
    message_type: &str,
    sequence: i64,
    extra: &[(&str, canonical::JcsValue<'_>)],
) -> Result<String> {
    let mut members = vec![
        ("message_id", canonical::JcsValue::Str(message_id)),
        ("message_type", canonical::JcsValue::Str(message_type)),
        ("sequence", canonical::JcsValue::Int(sequence)),
    ];
    members.extend_from_slice(extra);
    Ok(canonical::jcs_object(&members)?)
}

// -----------------------------------------------------------------------------------------------------------
// The inbound side: the durable inbox, the receipt, and the processing spine (DEC-059).
// -----------------------------------------------------------------------------------------------------------

/// An envelope that arrived from a peer, as the plain data this crate can store.
///
/// This crate deliberately does not depend on `crates/protocol`, so the bus hands the fields over rather than
/// the envelope: the boundary between "what an envelope means" and "what a row holds" stays where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    pub message_id: String,
    pub event_id: String,
    pub project_id: String,
    pub session_id: String,
    pub message_type: String,
    pub channel: String,
    /// The sender's ordering position within `(session_id, channel)`, which both directions share.
    pub sequence: i64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub envelope_json: String,
    pub created_at: String,
}

/// What the durable inbox knows about a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxRow {
    pub message_id: String,
    pub received_at: String,
    pub persisted_at: String,
    pub acked_at: Option<String>,
    pub processing_state: String,
    pub terminal_event_id: Option<String>,
}

/// What the durable receipt table knows about a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRow {
    pub receipt_id: String,
    pub message_id: String,
    pub project_id: String,
    pub receipt_state: String,
    pub acknowledged_at: Option<String>,
    pub created_at: String,
}

/// One receipt per message, derived rather than minted.
///
/// `message_receipts` declares no uniqueness on `message_id`, so the derivation is what makes the one-to-one
/// relation true rather than merely intended, and it is what lets the acknowledgement be updated to a
/// non-acknowledgement without a second row appearing.
fn receipt_id_for(message_id: &str) -> String {
    format!("rcpt_{message_id}")
}

/// Move a message along a declared edge, or refuse.
///
/// The `WHERE delivery_state = ?2` clause is the whole point: a transition that is not legal from the state the
/// row is actually in changes nothing, and the caller is told which state it found instead of being told that
/// zero rows were updated. This is what keeps the bus inside the machine even when its own bookkeeping is wrong.
fn advance_in(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    from: &str,
    to: &str,
    operation: &str,
) -> Result<()> {
    let changed = tx
        .execute(
            "UPDATE messages SET delivery_state = ?3 WHERE message_id = ?1 AND delivery_state = ?2",
            rusqlite::params![message_id, from, to],
        )
        .map_err(StorageError::Db)?;
    if changed == 1 {
        return Ok(());
    }
    let found: Option<String> = tx
        .query_row(
            "SELECT delivery_state FROM messages WHERE message_id = ?1",
            [message_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::Db)?;
    Err(StorageError::Malformed {
        column: "messages.delivery_state".to_string(),
        detail: format!(
            "{operation} requires {from}, found {}",
            found.unwrap_or_else(|| "no such message".to_string())
        ),
    })
}

/// A lifecycle event for a declared transition, with the fields every one of them carries.
#[allow(clippy::too_many_arguments)]
/// The id for a transition event that the machine may perform more than once for one message.
///
/// `transition_event_id` derives `evt_{message_id}_{event_type}`, which is deterministic and collides the moment
/// a message legitimately re-runs an edge - and a requeued message does exactly that, taking
/// `QUEUED -> DISPATCHED -> RECEIVED -> ACKED -> PROCESSING` a second time. `MESSAGE_QUEUED` is emitted by both
/// `enqueue` and `requeue`, and `MESSAGE_DISPATCHED` on every dispatch pass.
///
/// The id stays deterministic **from durable state**: the pass number is the count of events already recorded
/// for this message and event type, read inside the transaction that is about to append the next one. A crash and
/// retry therefore derives the same id rather than a new one, which is the property the plain derivation exists
/// to provide and which a random or timestamped id would lose.
///
/// The first occurrence keeps the plain id, so the ids of everything already written are unchanged and no
/// existing database's event log is renamed.
fn next_transition_event_id(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    event_type: &str,
) -> Result<String> {
    let base = transition_event_id(message_id, event_type);
    let prefix = format!("{base}_");
    // `instr(x, y) = 1` is "y is a prefix of x". `LIKE` would have been wrong here: the base contains
    // underscores, which `LIKE` treats as single-character wildcards, so the pattern would over-match.
    let seen: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_id = ?1 OR instr(event_id, ?2) = 1",
            rusqlite::params![base, prefix],
            |row| row.get(0),
        )
        .map_err(StorageError::Db)?;
    Ok(if seen == 0 {
        base
    } else {
        format!("{base}_{}", seen + 1)
    })
}

/// A lifecycle event for a declared transition, with the fields every one of them carries.
#[allow(clippy::too_many_arguments)]
fn transition_event(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    event_type: &str,
    project_id: &str,
    session_id: &str,
    correlation_id: &str,
    causation_id: Option<&str>,
    payload_json: String,
    created_at: &str,
) -> Result<NewEvent> {
    Ok(NewEvent {
        event_id: next_transition_event_id(tx, message_id, event_type)?,
        project_id: Some(project_id.to_string()),
        session_id: Some(session_id.to_string()),
        event_type: event_type.to_string(),
        correlation_id: Some(correlation_id.to_string()),
        causation_id: causation_id.map(str::to_string),
        // Every `message_delivery` transition record declares `epoch_effect: NONE`, so a lifecycle event carries
        // the epoch it was written under and does not change the project's.
        epoch: None,
        payload_json,
        created_at: created_at.to_string(),
    })
}

/// Take delivery of a message: persist its identity, then acknowledge receipt of it.
///
/// **Persist before acknowledge**, and both before any side effect. That order is what makes duplicate delivery
/// safe: the inbox row is the durable statement "this message has been seen", so a redelivery is answered from
/// it rather than processed twice, and a crash between the two leaves a message that is `RECEIVED` but not yet
/// `ACKED`, which the sender will send again and which the inbox will then recognise.
///
/// The row is born `DISPATCHED`. That is not a transition this crate performs: the message *arrived*, which
/// means its sender dispatched it, and the receiver's part of the machine begins where the sender's ended. The
/// two transitions written here are the receiver's own, and both are declared: `DISPATCHED -> RECEIVED`
/// (`MESSAGE_RECEIVED`) and `RECEIVED -> ACKED` (`MESSAGE_ACKED`). An acknowledgement is a receipt and not a
/// success (AGENTS.md section 7), which is why `ACKED` precedes `PROCESSING` rather than following it.
fn receive_message_in(
    tx: &rusqlite::Transaction<'_>,
    incoming: &IncomingMessage,
    now: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO messages (message_id, event_id, project_id, session_id, message_type, channel, sequence,
                               correlation_id, causation_id, idempotency_key, delivery_state, envelope_json,
                               created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'DISPATCHED', ?11, ?12)",
        rusqlite::params![
            incoming.message_id,
            incoming.event_id,
            incoming.project_id,
            incoming.session_id,
            incoming.message_type,
            incoming.channel,
            incoming.sequence,
            incoming.correlation_id,
            incoming.causation_id,
            incoming.idempotency_key,
            incoming.envelope_json,
            incoming.created_at,
        ],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "INSERT OR IGNORE INTO inbox (message_id, received_at, persisted_at, acked_at, processing_state, terminal_event_id)
         VALUES (?1, ?2, ?2, NULL, 'PERSISTED', NULL)",
        rusqlite::params![incoming.message_id, now],
    )
    .map_err(StorageError::Db)?;

    advance_in(
        tx,
        &incoming.message_id,
        "DISPATCHED",
        "RECEIVED",
        "a receive",
    )?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            &incoming.message_id,
            "MESSAGE_RECEIVED",
            &incoming.project_id,
            &incoming.session_id,
            &incoming.correlation_id,
            incoming.causation_id.as_deref(),
            transition_payload_fields(
                &incoming.message_id,
                &incoming.message_type,
                incoming.sequence,
            )?,
            now,
        )?,
    )?;

    advance_in(
        tx,
        &incoming.message_id,
        "RECEIVED",
        "ACKED",
        "an acknowledgement",
    )?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            &incoming.message_id,
            "MESSAGE_ACKED",
            &incoming.project_id,
            &incoming.session_id,
            &incoming.correlation_id,
            incoming.causation_id.as_deref(),
            transition_payload_fields(
                &incoming.message_id,
                &incoming.message_type,
                incoming.sequence,
            )?,
            now,
        )?,
    )?;
    tx.execute(
        "INSERT INTO message_receipts (receipt_id, message_id, project_id, receipt_state, acknowledged_at,
                                       created_at)
         VALUES (?1, ?2, ?3, 'ACKED', ?4, ?4)
         ON CONFLICT(receipt_id) DO UPDATE SET receipt_state = 'ACKED', acknowledged_at = ?4",
        rusqlite::params![
            receipt_id_for(&incoming.message_id),
            incoming.message_id,
            incoming.project_id,
            now
        ],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE inbox SET acked_at = ?2, processing_state = 'ACKED' WHERE message_id = ?1",
        rusqlite::params![incoming.message_id, now],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// `ACKED -> PROCESSING` (`ACTION_STARTED`), which is the boundary the side effect sits behind.
fn requeue_message_in(tx: &rusqlite::Transaction<'_>, message_id: &str, now: &str) -> Result<()> {
    let message = load_message_in(tx, message_id)?;
    advance_in(tx, message_id, "RETRYING", "QUEUED", "a requeue")?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            message_id,
            "MESSAGE_QUEUED",
            &message.project_id,
            &message.session_id,
            &message.correlation_id,
            message.causation_id.as_deref(),
            transition_payload_fields(message_id, &message.message_type, message.sequence)?,
            now,
        )?,
    )?;
    // The queue entry is upserted rather than updated, because a requeued message may not have one: a message
    // this bus received was dispatched by its peer, so it has an inbox row and no outbox row. Requeueing means
    // put it in the queue again, which requires a queue entry to exist.
    //
    // No backoff is applied, deliberately: the schema stores no processing-attempt count, so a delay would be
    // invented from nothing. Requeueing is a decision the caller takes.
    tx.execute(
        "INSERT INTO outbox (outbox_id, message_id, project_id, queued_at, dispatch_state, next_attempt_at,
                             attempts)
         VALUES (?1, ?2, ?3, ?4, 'PENDING', ?4, 0)
         ON CONFLICT(outbox_id) DO UPDATE SET dispatch_state = 'PENDING', next_attempt_at = ?4",
        rusqlite::params![outbox_id_for(message_id), message_id, message.project_id, now],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// ACKED -> PROCESSING (ACTION_STARTED), which is the boundary the side effect sits behind.
/// End a message that was refused retryably and never requeued: the declared `RETRYING -> EXPIRED` edge.
///
/// This is the last declared exit the bus did not implement. Without it a message refused retryably and not
/// requeued waited in `RETRYING` forever, with nothing recording that it was stuck - worse than a failure,
/// because a failure is visible.
///
/// Like requeue, this is a **decision the caller takes** rather than a timer the bus invents: the schema stores
/// no processing-attempt count, so there is no stored budget to derive an expiry from. The reason recorded is
/// therefore the budget itself rather than one particular refusal; the refusals themselves are in the event log,
/// which is where the reasons went in the first place.
///
/// The terminal state is `EXPIRED`, not `DEAD_LETTER`. `DEAD_LETTER` belongs to `PROCESSING -> REJECTED ->
/// DEAD_LETTER`, a receiver refusing a message outright; this is a message that could not be processed in time,
/// which is what `EXPIRED` names.
fn expire_retrying_in(tx: &rusqlite::Transaction<'_>, message_id: &str, now: &str) -> Result<()> {
    let message = load_message_in(tx, message_id)?;
    advance_in(tx, message_id, "RETRYING", "EXPIRED", "an expiry")?;
    let terminal_event = next_transition_event_id(tx, message_id, "MESSAGE_EXPIRED")?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            message_id,
            "MESSAGE_EXPIRED",
            &message.project_id,
            &message.session_id,
            &message.correlation_id,
            message.causation_id.as_deref(),
            transition_payload_fields(message_id, &message.message_type, message.sequence)?,
            now,
        )?,
    )?;
    tx.execute(
        "UPDATE outbox SET dispatch_state = 'ABANDONED', next_attempt_at = NULL WHERE message_id = ?1",
        [message_id],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "INSERT INTO dead_letters (dead_letter_id, message_id, project_id, final_error_json, attempts,
                                   created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(dead_letter_id) DO NOTHING",
        rusqlite::params![
            format!("dl_{message_id}"),
            message_id,
            message.project_id,
            canonical::jcs_object(&[
                ("code", canonical::JcsValue::Str("PROCESS_FAILED")),
                (
                    "detail",
                    canonical::JcsValue::Str("the processing budget was exhausted"),
                ),
            ])?,
            now
        ],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE inbox SET processing_state = 'EXPIRED', terminal_event_id = ?2 WHERE message_id = ?1",
        rusqlite::params![message_id, terminal_event],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// `ACKED -> PROCESSING` (`ACTION_STARTED`), which is the boundary the side effect sits behind.
fn start_processing_in(tx: &rusqlite::Transaction<'_>, message_id: &str, now: &str) -> Result<()> {
    let message = load_message_in(tx, message_id)?;
    advance_in(tx, message_id, "ACKED", "PROCESSING", "starting processing")?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            message_id,
            "ACTION_STARTED",
            &message.project_id,
            &message.session_id,
            &message.correlation_id,
            message.causation_id.as_deref(),
            transition_payload_fields(message_id, &message.message_type, message.sequence)?,
            now,
        )?,
    )?;
    tx.execute(
        "UPDATE inbox SET processing_state = 'PROCESSING' WHERE message_id = ?1",
        [message_id],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// `PROCESSING -> PROCESSED` (`ACTION_COMPLETED`), and the terminal event is recorded on the inbox row.
fn complete_processing_in(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    now: &str,
) -> Result<()> {
    let message = load_message_in(tx, message_id)?;
    let event_id = next_transition_event_id(tx, message_id, "ACTION_COMPLETED")?;
    advance_in(
        tx,
        message_id,
        "PROCESSING",
        "PROCESSED",
        "completing processing",
    )?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            message_id,
            "ACTION_COMPLETED",
            &message.project_id,
            &message.session_id,
            &message.correlation_id,
            message.causation_id.as_deref(),
            transition_payload_fields(message_id, &message.message_type, message.sequence)?,
            now,
        )?,
    )?;
    tx.execute(
        "UPDATE inbox SET processing_state = 'PROCESSED', terminal_event_id = ?2 WHERE message_id = ?1",
        rusqlite::params![message_id, event_id],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE message_receipts SET receipt_state = 'ACKED' WHERE message_id = ?1",
        [message_id],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// `PROCESSING -> RETRYING` (`ACTION_FAILED`, retryable) or `PROCESSING -> REJECTED -> DEAD_LETTER`
/// (`ACTION_FAILED` then `MESSAGE_DEAD_LETTERED`, not retryable).
///
/// **The reason travels in the event payload**, which is the durable record of why a decision was taken, and
/// for the terminal path it is also the dead letter's `final_error_json` under the registry's `PROCESS_FAILED`.
/// `message_receipts` has no column for a reason, and inventing one is not available: the table is fixed.
///
/// A non-retryable rejection reaches `DEAD_LETTER` through both of its declared edges rather than jumping to
/// the end. `DEAD_LETTER` is the receiver's terminal state, and it means the receiver refused a message it had
/// already received and had begun to process - which is exactly what happened here. A sender that never managed
/// to hand a message over never reaches it, because it was never refused.
fn reject_processing_in(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    reason: &str,
    retryable: bool,
    now: &str,
) -> Result<()> {
    let message = load_message_in(tx, message_id)?;
    let failure_payload = transition_payload_fields_and(
        message_id,
        &message.message_type,
        message.sequence,
        &[
            ("reason", canonical::JcsValue::Str(reason)),
            ("retryable", canonical::JcsValue::Bool(retryable)),
        ],
    )?;
    if retryable {
        advance_in(
            tx,
            message_id,
            "PROCESSING",
            "RETRYING",
            "a retryable rejection",
        )?;
    } else {
        advance_in(tx, message_id, "PROCESSING", "REJECTED", "a rejection")?;
    }
    append_event_in(
        tx,
        &transition_event(
            tx,
            message_id,
            "ACTION_FAILED",
            &message.project_id,
            &message.session_id,
            &message.correlation_id,
            message.causation_id.as_deref(),
            failure_payload,
            now,
        )?,
    )?;
    tx.execute(
        "UPDATE message_receipts SET receipt_state = 'NACKED' WHERE message_id = ?1",
        [message_id],
    )
    .map_err(StorageError::Db)?;

    if retryable {
        tx.execute(
            "UPDATE inbox SET processing_state = 'RETRYING' WHERE message_id = ?1",
            [message_id],
        )
        .map_err(StorageError::Db)?;
        return Ok(());
    }

    advance_in(tx, message_id, "REJECTED", "DEAD_LETTER", "dead-lettering")?;
    let terminal_event = next_transition_event_id(tx, message_id, "MESSAGE_DEAD_LETTERED")?;
    append_event_in(
        tx,
        &transition_event(
            tx,
            message_id,
            "MESSAGE_DEAD_LETTERED",
            &message.project_id,
            &message.session_id,
            &message.correlation_id,
            message.causation_id.as_deref(),
            transition_payload_fields_and(
                message_id,
                &message.message_type,
                message.sequence,
                &[
                    ("reason", canonical::JcsValue::Str(reason)),
                    ("retryable", canonical::JcsValue::Bool(false)),
                ],
            )?,
            now,
        )?,
    )?;
    tx.execute(
        "INSERT INTO dead_letters (dead_letter_id, message_id, project_id, final_error_json, attempts,
                                   created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        rusqlite::params![
            format!("dl_{message_id}"),
            message_id,
            message.project_id,
            canonical::jcs_object(&[
                ("code", canonical::JcsValue::Str("PROCESS_FAILED")),
                ("detail", canonical::JcsValue::Str(reason)),
            ])?,
            now
        ],
    )
    .map_err(StorageError::Db)?;
    tx.execute(
        "UPDATE inbox SET processing_state = 'DEAD_LETTER', terminal_event_id = ?2 WHERE message_id = ?1",
        rusqlite::params![message_id, terminal_event],
    )
    .map_err(StorageError::Db)?;
    Ok(())
}

/// The message a lifecycle event needs to describe, read inside the transaction that is about to describe it.
fn load_message_in(conn: &rusqlite::Connection, message_id: &str) -> Result<IncomingMessage> {
    conn.query_row(
        "SELECT message_id, event_id, project_id, session_id, message_type, channel, sequence,
                correlation_id, causation_id, idempotency_key, envelope_json, created_at
         FROM messages WHERE message_id = ?1",
        [message_id],
        |row| {
            Ok(IncomingMessage {
                message_id: row.get(0)?,
                event_id: row.get(1)?,
                project_id: row.get(2)?,
                session_id: row.get(3)?,
                message_type: row.get(4)?,
                channel: row.get(5)?,
                sequence: row.get(6)?,
                correlation_id: row.get(7)?,
                causation_id: row.get(8)?,
                idempotency_key: row.get(9)?,
                envelope_json: row.get(10)?,
                created_at: row.get(11)?,
            })
        },
    )
    .optional()
    .map_err(StorageError::Db)?
    .ok_or_else(|| StorageError::NotFound(format!("message {message_id}")))
}

/// The durable inbox row for a message, if it has ever been seen.
fn inbox_row_in(conn: &rusqlite::Connection, message_id: &str) -> Result<Option<InboxRow>> {
    conn.query_row(
        "SELECT message_id, received_at, persisted_at, acked_at, processing_state, terminal_event_id
         FROM inbox WHERE message_id = ?1",
        [message_id],
        |row| {
            Ok(InboxRow {
                message_id: row.get(0)?,
                received_at: row.get(1)?,
                persisted_at: row.get(2)?,
                acked_at: row.get(3)?,
                processing_state: row.get(4)?,
                terminal_event_id: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(StorageError::Db)
}

/// The receipt for a message, if one was written.
fn receipt_row_in(conn: &rusqlite::Connection, message_id: &str) -> Result<Option<ReceiptRow>> {
    conn.query_row(
        "SELECT receipt_id, message_id, project_id, receipt_state, acknowledged_at, created_at
         FROM message_receipts WHERE message_id = ?1",
        [message_id],
        |row| {
            Ok(ReceiptRow {
                receipt_id: row.get(0)?,
                message_id: row.get(1)?,
                project_id: row.get(2)?,
                receipt_state: row.get(3)?,
                acknowledged_at: row.get(4)?,
                created_at: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(StorageError::Db)
}

/// The inbound side of the bus, as storage operations.
///
/// Each of these is one transaction: the state mutation, the lifecycle event and the outbox-or-inbox effect
/// land together or not at all, which is what the transition records' `transaction_boundary` declares.
impl Storage {
    /// Take delivery of a message. The caller must have established that the message is not already known.
    pub fn receive_message(&mut self, incoming: &IncomingMessage, now: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        receive_message_in(&tx, incoming, now)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Begin processing a message that has been acknowledged.
    pub fn start_processing(&mut self, message_id: &str, now: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        start_processing_in(&tx, message_id, now)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Finish processing a message successfully.
    pub fn complete_processing(&mut self, message_id: &str, now: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        complete_processing_in(&tx, message_id, now)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Refuse a message that was being processed, retryably or terminally.
    pub fn reject_processing(
        &mut self,
        message_id: &str,
        reason: &str,
        retryable: bool,
        now: &str,
    ) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        reject_processing_in(&tx, message_id, reason, retryable, now)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Put a retryably refused message back in the queue.
    pub fn requeue_message(&mut self, message_id: &str, now: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        requeue_message_in(&tx, message_id, now)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// End a message that was refused retryably and never requeued: the declared `RETRYING -> EXPIRED` edge.
    pub fn expire_retrying(&mut self, message_id: &str, now: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        expire_retrying_in(&tx, message_id, now)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// The highest ordering position any message holds in a `(session_id, channel)` stream.
    ///
    /// This is what gap detection reads. It needs no column and no table of its own: the positions are already
    /// durable in `messages`, so "is a position missing?" is derived from the record rather than recorded
    /// separately - and a second record of the same fact is a second fact that can disagree.
    pub fn highest_sequence(&self, session_id: &str, channel: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT MAX(sequence) FROM messages WHERE session_id = ?1 AND channel = ?2",
                rusqlite::params![session_id, channel],
                |row| row.get(0),
            )
            .optional()
            .map(|found| found.flatten())
            .map_err(StorageError::Db)
    }

    /// How many of one project's messages sit in a given delivery state.
    ///
    /// The state is matched as a literal rather than validated against the machine's declared states: this is a
    /// count, and an unrecognised state truthfully counts zero instead of failing a read that changed nothing.
    pub fn message_count_in_state(&self, project_id: &str, delivery_state: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE project_id = ?1 AND delivery_state = ?2",
                rusqlite::params![project_id, delivery_state],
                |row| row.get(0),
            )
            .map_err(StorageError::Db)
    }

    /// How many dead letters one project has.
    ///
    /// Counted from `dead_letters`, which is the durable dead-letter record and the one that carries the final
    /// error and the attempt count.
    pub fn dead_letter_count(&self, project_id: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM dead_letters WHERE project_id = ?1",
                [project_id],
                |row| row.get(0),
            )
            .map_err(StorageError::Db)
    }

    /// The positions missing between observed positions in each of a project's `(session_id, channel)` streams.
    ///
    /// The walk starts at each stream's **lowest** observed position, matching the bus: a first message has
    /// nothing to be missing from, so positions below the first arrival are not gaps. `found` is never null,
    /// because the walk stops at the stream's highest position and that position is present by definition.
    pub fn open_sequence_gaps(&self, project_id: &str) -> Result<Vec<SequenceGapRow>> {
        let mut statement = self
            .conn
            .prepare(
                "WITH RECURSIVE
                   bounds(session_id, channel, lo, hi) AS (
                     SELECT session_id, channel, MIN(sequence), MAX(sequence)
                       FROM messages WHERE project_id = ?1
                      GROUP BY session_id, channel
                   ),
                   positions(session_id, channel, n, hi) AS (
                     SELECT session_id, channel, lo, hi FROM bounds
                     UNION ALL
                     SELECT session_id, channel, n + 1, hi FROM positions WHERE n < hi
                   )
                 SELECT p.session_id, p.channel, p.n,
                        (SELECT MIN(ahead.sequence) FROM messages ahead
                          WHERE ahead.project_id = ?1
                            AND ahead.session_id = p.session_id
                            AND ahead.channel = p.channel
                            AND ahead.sequence > p.n) AS found
                   FROM positions p
                   LEFT JOIN messages present
                     ON present.project_id = ?1
                    AND present.session_id = p.session_id
                    AND present.channel = p.channel
                    AND present.sequence = p.n
                  WHERE present.message_id IS NULL
                  ORDER BY p.session_id, p.channel, p.n",
            )
            .map_err(StorageError::Db)?;

        let rows = statement
            .query_map([project_id], |row| {
                Ok(SequenceGapRow {
                    session_id: row.get(0)?,
                    channel: row.get(1)?,
                    expected: row.get(2)?,
                    found: row.get(3)?,
                })
            })
            .map_err(StorageError::Db)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)
    }

    /// A consumer's durable position in a project's event stream, if it has one.
    ///
    /// `event_cursors` had no reader before this. Absence is returned as `None` rather than as zero, so the
    /// caller decides what an absent cursor means instead of receiving a number that looks durable.
    pub fn event_cursor_sequence(
        &self,
        project_id: &str,
        consumer_id: &str,
    ) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT last_sequence FROM event_cursors WHERE project_id = ?1 AND consumer_id = ?2",
                rusqlite::params![project_id, consumer_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Db)
    }

    /// How much dispatch work is still outstanding: queue entries whose message is still `QUEUED`.
    ///
    /// This is what a producer needs in order to throttle, and it is a read of the queue itself rather than a
    /// counter kept beside it. A message that has been handed over is `DISPATCHED`, and one that has terminated
    /// is `EXPIRED` or `DEAD_LETTER`; neither is work still to do, so neither is counted.
    pub fn pending_outbound_count(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM outbox o JOIN messages m ON m.message_id = o.message_id
                  WHERE o.dispatch_state IN ('PENDING', 'FAILED') AND m.delivery_state = 'QUEUED'",
                [],
                |row| row.get(0),
            )
            .map_err(StorageError::Db)
    }

    /// Everything needed to replay a terminal message as a new one.
    ///
    /// The envelope is rewritten with `json_set` rather than parsed and re-serialized: SQLite's JSON1 functions
    /// edit the stored document exactly, and string surgery on JSON would be a parser written by hand.
    ///
    /// Three fields change, and the protocol anticipates exactly these three - `envelope.rs` records that "a retry
    /// legitimately carries a new `message_id`, a new `sequence` and a new `created_at`":
    ///
    /// - `message_id`, because the original row keeps its identity and its terminal state;
    /// - `sequence`, because `(session_id, channel, sequence)` is unique and the original still holds its position,
    ///   so the replay takes the next free one rather than stealing the old one;
    /// - `created_at`, because this is a new event in the log and DEC-034's chain records when things happened,
    ///   not when the thing they retry happened.
    ///
    /// `causation_id` is set to the original's terminal event, so the log says why the new message exists rather
    /// than leaving it looking unrelated.
    pub fn replay_source(
        &self,
        message_id: &str,
        new_message_id: &str,
        now: &str,
    ) -> Result<ReplaySource> {
        let (envelope_json, project_id, session_id, channel, delivery_state) = self
            .conn
            .query_row(
                "SELECT envelope_json, project_id, session_id, channel, delivery_state FROM messages
                  WHERE message_id = ?1",
                [message_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(StorageError::Db)?
            .ok_or_else(|| StorageError::NotFound(format!("messages.message_id = {message_id}")))?;
        if delivery_state != "EXPIRED" && delivery_state != "DEAD_LETTER" {
            return Err(StorageError::NotTerminal {
                message_id: message_id.to_string(),
                delivery_state,
            });
        }
        // The next free position in the stream. The original keeps its own, so the replay cannot be a copy of it.
        let sequence: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE session_id = ?1 AND channel = ?2",
                rusqlite::params![session_id, channel],
                |row| row.get(0),
            )
            .map_err(StorageError::Db)?;
        let terminal_event_id: Option<String> = self
            .conn
            .query_row(
                "SELECT event_id FROM events
                  WHERE json_extract(payload_json, '$.message_id') = ?1
                    AND event_type IN ('MESSAGE_EXPIRED', 'MESSAGE_DEAD_LETTERED')
                  ORDER BY rowid DESC LIMIT 1",
                [message_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Db)?;
        let envelope_json: String = self
            .conn
            .query_row(
                "SELECT json_set(json_set(json_set(json_set(?1, '$.message_id', ?2), '$.sequence', ?3),
                                          '$.created_at', ?4), '$.causation_id', ?5)",
                rusqlite::params![envelope_json, new_message_id, sequence, now, terminal_event_id],
                |row| row.get(0),
            )
            .map_err(StorageError::Db)?;
        Ok(ReplaySource {
            envelope_json,
            project_id,
            sequence,
            terminal_event_id,
        })
    }

    /// The facts a replay decision needs, for one message.
    ///
    /// Read-only, and deliberately separate from `replay_source`, which rewrites the envelope: a caller that
    /// must judge before replaying has to be able to look without producing a replay.
    pub fn replay_facts(&self, message_id: &str) -> Result<ReplayFacts> {
        self.conn
            .query_row(
                "SELECT message_type,
                        json_extract(envelope_json, '$.context_snapshot_id'),
                        json_extract(envelope_json, '$.state_digest')
                   FROM messages WHERE message_id = ?1",
                [message_id],
                |row| {
                    Ok(ReplayFacts {
                        message_type: row.get(0)?,
                        context_snapshot_id: row.get(1)?,
                        state_digest: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(StorageError::Db)?
            .ok_or_else(|| StorageError::NotFound(format!("messages.message_id = {message_id}")))
    }

    /// A message's delivery state, which decides whether an arrival is a redelivery or a duplicate.
    pub fn delivery_state(&self, message_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT delivery_state FROM messages WHERE message_id = ?1",
                [message_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Db)
    }

    /// The durable inbox row for a message, which is how a redelivery is recognised.
    pub fn inbox_row(&self, message_id: &str) -> Result<Option<InboxRow>> {
        inbox_row_in(&self.conn, message_id)
    }

    /// The receipt for a message.
    pub fn receipt_row(&self, message_id: &str) -> Result<Option<ReceiptRow>> {
        receipt_row_in(&self.conn, message_id)
    }

    /// The message already holding a `(session_id, channel, sequence)` position, if any.
    ///
    /// Both directions share one ordering space per channel, so this is how the inbound side refuses to reuse a
    /// position the outbound side has taken rather than discovering it as a constraint violation.
    pub fn message_at_position(
        &self,
        session_id: &str,
        channel: &str,
        sequence: i64,
    ) -> Result<Option<String>> {
        find_message_at_position(&self.conn, session_id, channel, sequence)
    }
    /// Whether a project exists to route to.
    pub fn project_exists(&self, project_id: &str) -> Result<bool> {
        project_exists(&self.conn, project_id)
    }
    /// Whether a message id is already known, in either direction.
    pub fn message_exists(&self, message_id: &str) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
                [message_id],
                |row| row.get(0),
            )
            .map_err(StorageError::Db)?;
        Ok(count > 0)
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

        // A second connection that finds the database locked would otherwise fail at once: SQLite's default busy
        // timeout is zero, so a concurrent writer gets SQLITE_BUSY rather than waiting. Five seconds is long
        // enough for a competing write to commit and short enough that a genuine deadlock still surfaces as a
        // failure rather than a hang. Set per connection, which is why it belongs here and not in schema.sql.
        conn.execute_batch("PRAGMA busy_timeout = 5000;")
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

        // The genesis event of the project's chain. It goes through the same chain writer every later event
        // uses, so the first link is hashed by DEC-034's rule rather than carrying a placeholder: a sentinel
        // here would leave the one link no later event can repair permanently unverifiable.
        append_event_in(
            &tx,
            &NewEvent {
                event_id: new.event_id.clone(),
                project_id: Some(new.project_id.clone()),
                session_id: None,
                event_type: "PROJECT_CREATED".to_string(),
                correlation_id: None,
                causation_id: None,
                epoch: Some(0),
                payload_json: project_created_payload(new)?,
                created_at: new.created_at.clone(),
            },
        )?;

        tx.commit().map_err(StorageError::Db)?;

        Ok(CreatedProject {
            project_id: new.project_id.clone(),
            brief_id: new.brief_id.clone(),
            epoch: 0,
            phase: "DISCOVERY".to_string(),
            status: "ACTIVE".to_string(),
        })
    }

    /// Append one event to its chain, in its own transaction.
    ///
    /// The chain link is computed here from the current tail, so the caller supplies only the event's own
    /// fields. See [`append_event_in`] for the form that participates in a caller's transaction, which is what
    /// the transactional outbox requires: an event that is not committed with the state change it records is
    /// not a record of that change.
    pub fn append_event(&mut self, new: &NewEvent) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        append_event_in(&tx, new)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Recompute every event chain and report where it first stops verifying (DEC-034).
    ///
    /// This reads the whole `events` table, because that is what "recompute the chain from persisted rows"
    /// means. It reports; it never repairs.
    pub fn verify_event_chain(&self) -> Result<ChainVerification> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rowid, event_id, project_id, session_id, event_type, sequence,
                        correlation_id, causation_id, epoch, payload_json, created_at, prev_hash, event_hash
                 FROM events ORDER BY rowid",
            )
            .map_err(StorageError::Db)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ChainEvent {
                    rowid: r.get(0)?,
                    event_id: r.get(1)?,
                    project_id: r.get(2)?,
                    session_id: r.get(3)?,
                    event_type: r.get(4)?,
                    sequence: r.get(5)?,
                    correlation_id: r.get(6)?,
                    causation_id: r.get(7)?,
                    epoch: r.get(8)?,
                    payload_json: r.get(9)?,
                    created_at: r.get(10)?,
                    prev_hash: r.get(11)?,
                    event_hash: r.get(12)?,
                })
            })
            .map_err(StorageError::Db)?;
        let events = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Db)?;
        chain::verify(&events)
    }

    /// Persist an outbound message, queue it, and record both delivery transitions.
    ///
    /// One transaction covers the whole thing, because the implementation design requires a domain transaction
    /// to write its state mutation, its immutable event and its outbound record together. A message row whose
    /// queue entry is missing is a message nothing will ever send; an event that outlived a rolled-back state
    /// change is a record of something that did not happen.
    ///
    /// The two transitions are the `message_delivery` machine's own first two spine steps. `CREATED ->
    /// PERSISTED` writes the `messages` row; `PERSISTED -> QUEUED` writes the `outbox` row and advances the
    /// message. Both are declared transitions with their own canonical event, so both are recorded: collapsing
    /// them into one event would drop a declared transition out of the immutable history that is supposed to
    /// hold all of them. They are not separately observable, because they commit together.
    pub fn enqueue_message(&mut self, new: &NewOutboundMessage) -> Result<EnqueuedMessage> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        let enqueued = enqueue_message_in(&tx, new)?;
        tx.commit().map_err(StorageError::Db)?;
        Ok(enqueued)
    }

    /// The queued outbound message for a project operation, if one exists.
    ///
    /// The idempotency scope is `project_id + operation_id` (DEC-027, and the implementation design's
    /// "Idempotency scope"). `operation_id` is an optional envelope field with no column of its own, so it is
    /// read out of `envelope_json`; that is why the index this crate creates covers the extracted value rather
    /// than a column.
    pub fn find_outbound_by_operation(
        &self,
        project_id: &str,
        operation_id: &str,
    ) -> Result<Option<EnqueuedMessage>> {
        find_outbound_by_operation_in(&self.conn, project_id, operation_id)
    }

    /// The queue entries that are due at `now`, oldest first, at most `limit` of them.
    ///
    /// A read, not a claim: two dispatchers running at once would both see the same entries and both send them.
    /// That is deliberate and it is safe, because duplicate delivery is the condition the durable inbox exists
    /// to absorb - the design says a crash "may cause duplicate delivery and must be safe" - and because a claim
    /// would need a lease column this schema does not have. What is *not* safe is a second dispatcher's attempt
    /// row racing the first's; each attempt is written in its own transaction under the attempt number the
    /// decision produced, and `message_attempts.attempt_id` is the primary key, so a collision is refused rather
    /// than silently merged.
    pub fn due_outbound(&self, now: &str, limit: i64) -> Result<Vec<DueMessage>> {
        due_outbound_in(&self.conn, now, limit)
    }

    /// Record a failed delivery attempt and schedule the next one. The message stays `QUEUED`.
    pub fn record_failed_attempt(
        &mut self,
        message_id: &str,
        attempt: &DeliveryAttempt,
        backoff_seconds: i64,
    ) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        record_failed_attempt_in(&tx, message_id, attempt, backoff_seconds)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Record a successful handover and advance the message `QUEUED -> DISPATCHED`.
    pub fn record_dispatched(&mut self, due: &DueMessage, attempt: &DeliveryAttempt) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        record_dispatched_in(&tx, due, attempt)?;
        tx.commit().map_err(StorageError::Db)
    }

    /// Terminate an exhausted message through `QUEUED -> EXPIRED`, with a dead letter.
    pub fn expire_message(&mut self, due: &DueMessage, finished_at: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        expire_message_in(&tx, due, finished_at)?;
        tx.commit().map_err(StorageError::Db)
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

        // Durable history is a recovery question: "has the record been altered?" is exactly what a startup
        // scan should answer, and the chain is the only thing that can answer it. Reported here rather than
        // left to a separate call so a clean recovery report means history verified too.
        let chain = self.verify_event_chain()?;
        if let Some(first) = chain.first_divergence() {
            let position = match first.sequence {
                Some(sequence) => format!("sequence {sequence}"),
                None => "no sequence".to_string(),
            };
            let hashes = match (&first.expected_hash, &first.stored_hash) {
                (Some(expected), Some(stored)) => {
                    format!("; chain requires {expected}, row carries {stored}")
                }
                _ => String::new(),
            };
            issues.push(RecoveryIssue {
                kind: "EVENT_CHAIN_BROKEN",
                detail: format!(
                    "event {} ({position}) fails {}: {} of {} event(s) across {} chain(s) did not verify{hashes}",
                    first.event_id,
                    first.kind.code(),
                    chain.divergences.len(),
                    chain.events,
                    chain.chains,
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
fn agent_session_event_payload(
    project_id: &str,
    session_id: &str,
    agent_id: &str,
    state: &str,
) -> Result<String> {
    canonical::jcs_object(&[
        ("project_id", canonical::JcsValue::Str(project_id)),
        ("session_id", canonical::JcsValue::Str(session_id)),
        ("agent_id", canonical::JcsValue::Str(agent_id)),
        ("state", canonical::JcsValue::Str(state)),
    ])
}

fn task_lease_task_event_payload(
    project_id: &str,
    task_id: &str,
    lease_id: &str,
    state: &str,
) -> Result<String> {
    canonical::jcs_object(&[
        ("project_id", canonical::JcsValue::Str(project_id)),
        ("task_id", canonical::JcsValue::Str(task_id)),
        ("lease_id", canonical::JcsValue::Str(lease_id)),
        ("state", canonical::JcsValue::Str(state)),
    ])
}

fn lease_event_payload(
    project_id: &str,
    task_id: &str,
    lease_id: &str,
    lease_version: i64,
    state: &str,
) -> Result<String> {
    canonical::jcs_object(&[
        ("project_id", canonical::JcsValue::Str(project_id)),
        ("task_id", canonical::JcsValue::Str(task_id)),
        ("lease_id", canonical::JcsValue::Str(lease_id)),
        ("lease_version", canonical::JcsValue::Int(lease_version)),
        ("state", canonical::JcsValue::Str(state)),
    ])
}

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

/// Durable retry/recovery identity for a Task. The active lease version is copied as the fencing value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulableTask {
    pub task_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub priority: i64,
    pub risk: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInstallationRecord {
    pub agent_id: String,
    pub agent_type: String,
    pub executable: String,
    pub resolved_path: Option<String>,
    pub version: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewAgentSession {
    pub session_id: String,
    pub project_id: String,
    pub agent_id: String,
    pub workspace_id: Option<String>,
    pub current_epoch: i64,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionRecord {
    pub session_id: String,
    pub project_id: String,
    pub agent_id: String,
    pub native_session_id: Option<String>,
    pub state: String,
    pub health_state: String,
    pub process_id: Option<i64>,
    pub workspace_id: Option<String>,
    pub capability_snapshot_id: Option<String>,
    pub current_epoch: i64,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentCapabilitySnapshot {
    pub capability_snapshot_id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub capabilities_json: String,
    pub detected_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTaskLease {
    pub lease_id: String,
    pub task_id: String,
    pub project_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub project_epoch: i64,
    pub context_snapshot_id: String,
    pub state_digest: String,
    pub allowed_paths_json: String,
    pub required_capabilities_json: String,
    pub policy_scope: String,
    pub issued_at: String,
    pub heartbeat_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLeaseRecord {
    pub lease_id: String,
    pub task_id: String,
    pub project_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub lease_version: i64,
    pub project_epoch: i64,
    pub context_snapshot_id: String,
    pub state_digest: String,
    pub allowed_paths_json: String,
    pub required_capabilities_json: String,
    pub policy_scope: String,
    pub issued_at: String,
    pub heartbeat_at: String,
    pub expires_at: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommandExecution {
    pub execution_id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub workspace_id: String,
    pub requested_by_agent_id: Option<String>,
    pub environment_snapshot_id: Option<String>,
    pub supersedes_binding_id: Option<String>,
    pub classification: String,
    pub executable: String,
    pub arguments_json: String,
    pub cwd: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub timeout_seconds: i64,
    pub stdout_artifact_id: Option<String>,
    pub stderr_artifact_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExecutionRecord {
    pub execution_id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub workspace_id: String,
    pub requested_by_agent_id: Option<String>,
    pub environment_snapshot_id: Option<String>,
    pub supersedes_binding_id: Option<String>,
    pub classification: String,
    pub executable: String,
    pub arguments_json: String,
    pub cwd: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub timeout_seconds: i64,
    pub stdout_artifact_id: Option<String>,
    pub stderr_artifact_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProcessRecord {
    pub process_record_id: String,
    pub execution_id: String,
    pub pid: i64,
    pub parent_pid: Option<i64>,
    pub state: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRecord {
    pub process_record_id: String,
    pub execution_id: String,
    pub pid: i64,
    pub parent_pid: Option<i64>,
    pub state: String,
    pub observed_at: String,
}

pub struct NewTaskAttempt {
    pub attempt_id: String,
    pub task_id: String,
    pub project_id: String,
    pub attempt_no: i64,
    pub lease_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub fence_token: i64,
    pub project_epoch: i64,
    pub context_snapshot_id: String,
    pub state: String,
    pub checkpoint_id: Option<String>,
    pub failure_id: Option<String>,
    pub started_at: Option<String>,
    pub heartbeat_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewResourceReservation {
    pub reservation_id: String,
    pub project_id: String,
    pub task_id: String,
    pub lease_id: String,
    pub lease_version: i64,
    pub resource_type: String,
    pub resource_key: String,
    pub mode: String,
    pub quantity: i64,
    pub state: String,
    pub issued_at: String,
    pub expires_at: String,
    pub released_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkspaceRevision {
    pub revision_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub revision_no: i64,
    pub parent_revision_id: Option<String>,
    pub repository_head: Option<String>,
    pub manifest_hash: Option<String>,
    pub tree_hash: Option<String>,
    pub source: String,
    pub status: String,
    pub observed_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEnvironmentSnapshot {
    pub environment_snapshot_id: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub task_id: Option<String>,
    pub execution_id: Option<String>,
    pub os_identity: String,
    pub runtime_versions_json: String,
    pub environment_policy_hash: String,
    pub source: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificationBindingRecord {
    pub certification_binding_id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub validation_id: String,
    pub workspace_revision_id: Option<String>,
    pub environment_snapshot_id: Option<String>,
    pub artifact_hashes_json: String,
    pub validator_version: String,
    pub test_suite_version: Option<String>,
    pub status: String,
    pub supersedes_binding_id: Option<String>,
    pub reason: Option<String>,
    pub created_at: String,
}

pub struct NewCertificationBinding {
    pub certification_binding_id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub validation_id: String,
    pub workspace_revision_id: Option<String>,
    pub environment_snapshot_id: Option<String>,
    pub artifact_hashes_json: String,
    pub validator_version: String,
    pub test_suite_version: Option<String>,
    pub status: String,
    pub supersedes_binding_id: Option<String>,
    pub reason: Option<String>,
    pub created_at: String,
}

const TASK_ATTEMPT_STATES: &[&str] = &["CREATED","STARTED","RUNNING","CHECKPOINTED","COMPLETED","FAILED","TIMED_OUT","LOST","CANCELLED","UNKNOWN"];
const EXECUTION_CLASSIFICATIONS: &[&str] = &["READ_ONLY","SAFE_WRITE","PROJECT_WRITE","EXECUTE","INSTALL","ADMIN_REQUIRED"];
const LEASE_STATES: &[&str] = &["REQUESTED","ACTIVE","RENEWING","EXPIRED","RELEASED","REVOKED"];
const AGENT_SESSION_STATES: &[&str] = &["DISCOVERED","HANDSHAKING","CAPABILITY_VALIDATING","WORKSPACE_VALIDATING","READY","ACTIVE","PAUSED","DRAINING","STOPPED","LOST","RECONNECTING","SYNCING","FAILED"];
const AGENT_HEALTH_STATES: &[&str] = &["HEALTHY","DEGRADED","UNHEALTHY","UNKNOWN"];
const EXECUTION_STATES: &[&str] = &["REQUESTED","POLICY_CHECK","APPROVED","STARTING","RUNNING","EXITED","EVIDENCE_CAPTURED","RECORDED","DENIED","TIMEOUT","CANCELED","CRASHED","CLEANUP_REQUIRED"];
const PROCESS_STATES: &[&str] = &["EXPECTED","VERIFIED","DRIFTED","UNKNOWN"];
const RESOURCE_TYPES: &[&str] = &["CPU","RAM","GPU","DISK","PORT","WORKSPACE","PROCESS_SLOT","AGENT_SLOT","TOOLCHAIN"];
const RESOURCE_MODES: &[&str] = &["EXCLUSIVE","SHARED"];
const RESOURCE_STATES: &[&str] = &["HELD","RELEASED","EXPIRED","LOST"];
const REVISION_SOURCES: &[&str] = &["CONTROLLER","AGENT","USER","EXTERNAL","GIT"];
const REVISION_STATES: &[&str] = &["EXPECTED","VERIFIED","DRIFTED","UNKNOWN"];
const CERTIFICATION_STATES: &[&str] = &["ASSERTED","INVALIDATED","EXPIRED"];

impl Storage {
    /// Admit and activate one task lease in a single transaction.
    ///
    /// The request state is recorded before activation inside the same transaction so durable history contains
    /// the canonical REQUESTED -> ACTIVE transition, while no caller can observe an intermediate half-granted
    /// lease after a crash.
    /// Persist a new agent session at DISCOVERED. The session is the runtime identity anchor for a single process lineage.
    /// Persist the latest local installation fact for one of the closed agent types.
    /// This table is runtime discovery state; it is not a second configuration authority.
    pub fn upsert_agent_installation(
        &self,
        agent_id: &str,
        agent_type: &str,
        executable: &str,
        resolved_path: Option<&str>,
        version: Option<&str>,
        updated_at: &str,
    ) -> Result<AgentInstallationRecord> {
        if !matches!(agent_type, "HERMES_AGENT" | "KILO_CODE" | "OPEN_CODE") {
            return Err(StorageError::Malformed {
                column: "agents.agent_type".to_string(),
                detail: format!("unsupported agent type {agent_type}"),
            });
        }
        if executable.trim().is_empty() || updated_at.trim().is_empty() {
            return Err(StorageError::Malformed {
                column: "agents".to_string(),
                detail: "executable and updated_at are required".to_string(),
            });
        }
        self.conn.execute(
            "INSERT INTO agents (agent_id,agent_type,executable,resolved_path,version,enabled,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,1,?6,?6)
             ON CONFLICT(agent_id) DO UPDATE SET agent_type=excluded.agent_type, executable=excluded.executable,
                 resolved_path=excluded.resolved_path, version=excluded.version, enabled=1, updated_at=excluded.updated_at",
            rusqlite::params![agent_id,agent_type,executable,resolved_path,version,updated_at],
        ).map_err(StorageError::Db)?;
        self.conn.query_row(
            "SELECT agent_id,agent_type,executable,resolved_path,version,enabled,created_at,updated_at
             FROM agents WHERE agent_id=?1",
            [agent_id],
            |r| Ok(AgentInstallationRecord {
                agent_id:r.get(0)?,agent_type:r.get(1)?,executable:r.get(2)?,resolved_path:r.get(3)?,
                version:r.get(4)?,enabled:r.get::<_,i64>(5)? != 0,created_at:r.get(6)?,updated_at:r.get(7)?
            }),
        ).map_err(StorageError::Db)
    }

    pub fn create_agent_session(&mut self, new: &NewAgentSession) -> Result<AgentSessionRecord> {
        if new.current_epoch < 0 || new.started_at.trim().is_empty() {
            return Err(StorageError::Malformed {
                column: "agent_sessions".to_string(),
                detail: "current_epoch must be non-negative and started_at must be non-empty".to_string(),
            });
        }
        let agent_project_ok: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM agents WHERE agent_id=?1 AND enabled=1",
            [new.agent_id.as_str()],
            |r| r.get(0),
        ).map_err(StorageError::Db)?;
        if agent_project_ok != 1 {
            return Err(StorageError::NotFound(format!("enabled agent {}", new.agent_id)));
        }
        let project_exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM projects WHERE project_id=?1",
            [new.project_id.as_str()],
            |r| r.get(0),
        ).map_err(StorageError::Db)?;
        if project_exists != 1 {
            return Err(StorageError::NotFound(format!("project {}", new.project_id)));
        }
        if let Some(workspace_id) = new.workspace_id.as_deref() {
            let exists: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM workspaces WHERE workspace_id=?1 AND project_id=?2",
                rusqlite::params![workspace_id,new.project_id],
                |r| r.get(0),
            ).map_err(StorageError::Db)?;
            if exists != 1 {
                return Err(StorageError::Malformed {
                    column: "agent_sessions.workspace_id".to_string(),
                    detail: "workspace does not belong to project".to_string(),
                });
            }
        }

        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        tx.execute(
            "INSERT INTO agent_sessions (session_id,project_id,agent_id,native_session_id,state,health_state,process_id,workspace_id,capability_snapshot_id,current_epoch,started_at,stopped_at)
             VALUES (?1,?2,?3,NULL,'DISCOVERED','UNKNOWN',NULL,?4,NULL,?5,?6,NULL)",
            rusqlite::params![new.session_id,new.project_id,new.agent_id,new.workspace_id,new.current_epoch,new.started_at],
        ).map_err(StorageError::Db)?;
        append_event_in(&tx, &transition_event(
            &tx,
            &new.session_id,
            "AGENT_DISCOVERED",
            &new.project_id,
            &new.session_id,
            &new.session_id,
            None,
            agent_session_event_payload(&new.project_id,&new.session_id,&new.agent_id,"DISCOVERED")?,
            &new.started_at,
        )?)?;
        tx.commit().map_err(StorageError::Db)?;
        self.get_agent_session(&new.session_id)?
            .ok_or_else(|| StorageError::NotFound(format!("agent session {}", new.session_id)))
    }

    pub fn get_agent_session(&self, session_id: &str) -> Result<Option<AgentSessionRecord>> {
        self.conn.query_row(
            "SELECT session_id,project_id,agent_id,native_session_id,state,health_state,process_id,workspace_id,capability_snapshot_id,current_epoch,started_at,stopped_at
             FROM agent_sessions WHERE session_id=?1",
            [session_id],
            |r| Ok(AgentSessionRecord {
                session_id:r.get(0)?, project_id:r.get(1)?, agent_id:r.get(2)?,
                native_session_id:r.get(3)?, state:r.get(4)?, health_state:r.get(5)?,
                process_id:r.get(6)?, workspace_id:r.get(7)?, capability_snapshot_id:r.get(8)?,
                current_epoch:r.get(9)?, started_at:r.get(10)?, stopped_at:r.get(11)?
            }),
        ).optional().map_err(StorageError::Db)
    }

    /// CAS transition for the agent-session machine. The event is emitted in the same transaction as the state mutation.
    pub fn transition_agent_session(
        &mut self,
        session_id: &str,
        expected_state: &str,
        next_state: &str,
        event_type: &str,
        now: &str,
    ) -> Result<()> {
        require_vocabulary("agent_sessions.expected_state", expected_state, AGENT_SESSION_STATES)?;
        require_vocabulary("agent_sessions.next_state", next_state, AGENT_SESSION_STATES)?;
        let legal = matches!(
            (expected_state,next_state),
            ("DISCOVERED","HANDSHAKING")
            | ("HANDSHAKING","CAPABILITY_VALIDATING")
            | ("CAPABILITY_VALIDATING","WORKSPACE_VALIDATING")
            | ("WORKSPACE_VALIDATING","READY")
            | ("READY","ACTIVE")
            | ("ACTIVE","PAUSED")
            | ("PAUSED","DRAINING")
            | ("DRAINING","STOPPED")
            | ("READY","LOST")
            | ("ACTIVE","LOST")
            | ("LOST","RECONNECTING")
            | ("RECONNECTING","SYNCING")
            | ("SYNCING","READY")
            | ("SYNCING","ACTIVE")
            | ("SYNCING","FAILED")
        );
        if !legal {
            return Err(StorageError::Malformed {
                column: "agent_sessions.state".to_string(),
                detail: format!("illegal agent-session transition {expected_state} -> {next_state}"),
            });
        }
        let expected_event = match next_state {
            "HANDSHAKING" => "AGENT_HANDSHAKING",
            "CAPABILITY_VALIDATING" => "AGENT_CAPABILITY_VALIDATING",
            "WORKSPACE_VALIDATING" => "AGENT_WORKSPACE_VALIDATING",
            "READY" => "AGENT_READY",
            "ACTIVE" => "AGENT_ACTIVATED",
            "PAUSED" => "AGENT_PAUSED",
            "DRAINING" => "AGENT_DRAINING",
            "STOPPED" => "AGENT_STOPPED",
            "LOST" => "AGENT_LOST",
            "RECONNECTING" => "AGENT_RECONNECTING",
            "SYNCING" => "AGENT_SYNCED",
            "FAILED" => "AGENT_FAILED",
            "DISCOVERED" => "AGENT_DISCOVERED",
            _ => "AGENT_SESSION_CHANGED",
        };
        if event_type != expected_event {
            return Err(StorageError::Malformed {
                column: "agent_sessions.event_type".to_string(),
                detail: format!("event {event_type} does not enter target state {next_state}; expected {expected_event}"),
            });
        }

        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        let row: Option<(String,String,String,String)> = tx.query_row(
            "SELECT project_id,agent_id,state,health_state FROM agent_sessions WHERE session_id=?1",
            [session_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional().map_err(StorageError::Db)?;
        let (project_id,agent_id,current,_) =
            row.ok_or_else(|| StorageError::NotFound(format!("agent session {session_id}")))?;
        if current != expected_state {
            return Err(StorageError::Malformed {
                column: "agent_sessions.state".to_string(),
                detail: format!("compare-and-swap failed: expected {expected_state}, found {current}"),
            });
        }
        let changed = tx.execute(
            "UPDATE agent_sessions SET state=?1 WHERE session_id=?2 AND state=?3",
            rusqlite::params![next_state,session_id,expected_state],
        ).map_err(StorageError::Db)?;
        if changed != 1 {
            return Err(StorageError::Malformed {
                column: "agent_sessions.state".to_string(),
                detail: "agent-session state changed concurrently".to_string(),
            });
        }
        append_event_in(&tx, &transition_event(
            &tx,
            session_id,
            event_type,
            &project_id,
            session_id,
            session_id,
            None,
            agent_session_event_payload(&project_id,session_id,&agent_id,next_state)?,
            now,
        )?)?;
        tx.commit().map_err(StorageError::Db)?;
        Ok(())
    }

    pub fn bind_agent_process(
        &self,
        session_id: &str,
        process_id: i64,
        native_session_id: Option<&str>,
    ) -> Result<()> {
        if process_id <= 0 {
            return Err(StorageError::Malformed {
                column: "agent_sessions.process_id".to_string(),
                detail: "process_id must be positive".to_string(),
            });
        }
        let changed = self.conn.execute(
            "UPDATE agent_sessions SET process_id=?1,native_session_id=COALESCE(?2,native_session_id) WHERE session_id=?3",
            rusqlite::params![process_id,native_session_id,session_id],
        ).map_err(StorageError::Db)?;
        if changed != 1 {
            return Err(StorageError::NotFound(format!("agent session {session_id}")));
        }
        Ok(())
    }

    pub fn set_agent_health(
        &self,
        session_id: &str,
        health_state: &str,
    ) -> Result<()> {
        require_vocabulary("agent_sessions.health_state", health_state, AGENT_HEALTH_STATES)?;
        let changed = self.conn.execute(
            "UPDATE agent_sessions SET health_state=?1 WHERE session_id=?2",
            rusqlite::params![health_state,session_id],
        ).map_err(StorageError::Db)?;
        if changed != 1 {
            return Err(StorageError::NotFound(format!("agent session {session_id}")));
        }
        Ok(())
    }

    pub fn insert_agent_capability_snapshot(&self, new: &NewAgentCapabilitySnapshot) -> Result<()> {
        let caps: serde_json::Value = serde_json::from_str(&new.capabilities_json).map_err(|e| StorageError::Malformed {
            column:"agent_capabilities.capabilities_json".to_string(),
            detail:format!("must be valid JSON object: {e}"),
        })?;
        if !caps.is_object() {
            return Err(StorageError::Malformed {
                column:"agent_capabilities.capabilities_json".to_string(),
                detail:"capabilities_json must be a JSON object".to_string(),
            });
        }
        if let Some(session_id)=new.session_id.as_deref() {
            let matches:i64=self.conn.query_row(
                "SELECT COUNT(*) FROM agent_sessions WHERE session_id=?1 AND agent_id=?2",
                rusqlite::params![session_id,new.agent_id],
                |r| r.get(0),
            ).map_err(StorageError::Db)?;
            if matches!=1 {
                return Err(StorageError::Malformed {
                    column:"agent_capabilities.session_id".to_string(),
                    detail:"capability session does not belong to agent".to_string(),
                });
            }
        }
        self.conn.execute(
            "INSERT INTO agent_capabilities (capability_snapshot_id,agent_id,session_id,capabilities_json,detected_at)
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![new.capability_snapshot_id,new.agent_id,new.session_id,new.capabilities_json,new.detected_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    pub fn lease_task(&mut self, new: &NewTaskLease) -> Result<TaskLeaseRecord> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        let task: Option<(String,Option<String>,i64,String)> = tx.query_row(
            "SELECT project_id, workspace_id, current_epoch, status FROM tasks WHERE task_id=?1",
            [new.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional().map_err(StorageError::Db)?;
        let (task_project, task_workspace, task_epoch, task_status) =
            task.ok_or_else(|| StorageError::NotFound(format!("task {}", new.task_id)))?;
        if task_project != new.project_id
            || task_workspace.as_deref() != Some(new.workspace_id.as_str())
            || task_epoch != new.project_epoch
        {
            return Err(StorageError::Malformed {
                column: "task_leases".to_string(),
                detail: "lease does not match task project/workspace/epoch".to_string(),
            });
        }
        if !matches!(task_status.as_str(), "READY" | "LEASE_REQUESTED") {
            return Err(StorageError::Malformed {
                column: "tasks.status".to_string(),
                detail: format!("task {} is not leaseable from {}", new.task_id, task_status),
            });
        }

        let agent_matches: i64 = tx.query_row(
            "SELECT COUNT(*) FROM agents WHERE agent_id=?1 AND enabled=1",
            [new.agent_id.as_str()],
            |row| row.get(0),
        ).map_err(StorageError::Db)?;
        let session: Option<(String,String,String,String,i64)> = tx.query_row(
            "SELECT project_id, agent_id, state, health_state, current_epoch FROM agent_sessions WHERE session_id=?1",
            [new.session_id.as_str()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).optional().map_err(StorageError::Db)?;
        if agent_matches != 1 {
            return Err(StorageError::NotFound(format!("enabled agent {}", new.agent_id)));
        }
        let (session_project, session_agent, session_state, health, session_epoch) =
            session.ok_or_else(|| StorageError::NotFound(format!("agent session {}", new.session_id)))?;
        if session_project != new.project_id
            || session_agent != new.agent_id
            || !matches!(session_state.as_str(), "READY" | "ACTIVE")
            || health != "HEALTHY"
            || session_epoch != new.project_epoch
        {
            return Err(StorageError::Malformed {
                column: "agent_sessions".to_string(),
                detail: format!("agent session {} is not eligible for lease admission", new.session_id),
            });
        }

        let context: Option<(String,i64,Option<String>)> = tx.query_row(
            "SELECT project_id, epoch, invalidated_at FROM context_snapshots WHERE context_snapshot_id=?1",
            [new.context_snapshot_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().map_err(StorageError::Db)?;
        let (context_project, context_epoch, invalidated_at) = context.ok_or_else(|| {
            StorageError::NotFound(format!("context snapshot {}", new.context_snapshot_id))
        })?;
        if context_project != new.project_id || context_epoch != new.project_epoch || invalidated_at.is_some() {
            return Err(StorageError::Malformed {
                column: "task_leases.context_snapshot_id".to_string(),
                detail: "context snapshot is missing, stale for the task epoch, or invalidated".to_string(),
            });
        }

        let existing: i64 = tx.query_row(
            "SELECT COUNT(*) FROM task_leases WHERE task_id=?1 AND status IN ('ACTIVE','RENEWING')",
            [new.task_id.as_str()],
            |row| row.get(0),
        ).map_err(StorageError::Db)?;
        if existing != 0 {
            return Err(StorageError::Malformed {
                column: "task_leases.task_id".to_string(),
                detail: format!("task {} already has a live lease", new.task_id),
            });
        }

        for (column, json_field) in [
            ("task_leases.allowed_paths_json", &new.allowed_paths_json),
            ("task_leases.required_capabilities_json", &new.required_capabilities_json),
        ] {
            let value: serde_json::Value = serde_json::from_str(json_field).map_err(|e| StorageError::Malformed {
                column: column.to_string(),
                detail: format!("must be valid JSON: {e}"),
            })?;
            if !value.is_array() {
                return Err(StorageError::Malformed {
                    column: column.to_string(),
                    detail: "must be a JSON array".to_string(),
                });
            }
        }
        if new.issued_at.trim().is_empty()
            || new.heartbeat_at.trim().is_empty()
            || new.expires_at.trim().is_empty()
            || new.state_digest.trim().is_empty()
            || new.policy_scope.trim().is_empty()
        {
            return Err(StorageError::Malformed {
                column: "task_leases".to_string(),
                detail: "lease timestamps, state_digest and policy_scope are required".to_string(),
            });
        }

        // Make the task's lease-request transition and the lease's request/grant transitions one SQLite transaction.
        // The event IDs use different identities so TASK and LEASE lifecycle events cannot collide.
        if task_status == "READY" {
            tx.execute(
                "UPDATE tasks SET status='LEASE_REQUESTED', updated_at=?1 WHERE task_id=?2 AND status='READY'",
                rusqlite::params![new.issued_at, new.task_id],
            ).map_err(StorageError::Db)?;
            append_event_in(&tx, &transition_event(
                &tx,
                &format!("task_{}", new.task_id),
                "LEASE_REQUESTED",
                &new.project_id,
                &new.session_id,
                &new.lease_id,
                None,
                task_lease_task_event_payload(&new.project_id, &new.task_id, &new.lease_id, "LEASE_REQUESTED")?,
                &new.issued_at,
            )?)?;
        }

        tx.execute(
            "INSERT INTO task_leases (lease_id,task_id,project_id,agent_id,session_id,workspace_id,lease_version,project_epoch,context_snapshot_id,state_digest,allowed_paths_json,required_capabilities_json,policy_scope,issued_at,heartbeat_at,expires_at,status)
             VALUES (?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?10,?11,?12,?13,?14,?15,'REQUESTED')",
            rusqlite::params![
                new.lease_id,new.task_id,new.project_id,new.agent_id,new.session_id,new.workspace_id,new.project_epoch,
                new.context_snapshot_id,new.state_digest,new.allowed_paths_json,new.required_capabilities_json,new.policy_scope,
                new.issued_at,new.heartbeat_at,new.expires_at
            ],
        ).map_err(StorageError::Db)?;

        append_event_in(&tx, &transition_event(
            &tx,
            &format!("lease_{}", new.lease_id),
            "LEASE_REQUESTED",
            &new.project_id,
            &new.session_id,
            &new.lease_id,
            None,
            lease_event_payload(&new.project_id,&new.task_id,&new.lease_id,1,"REQUESTED")?,
            &new.issued_at,
        )?)?;

        tx.execute(
            "UPDATE task_leases SET status='ACTIVE' WHERE lease_id=?1 AND status='REQUESTED'",
            [new.lease_id.as_str()],
        ).map_err(StorageError::Db)?;
        append_event_in(&tx, &transition_event(
            &tx,
            &format!("lease_{}", new.lease_id),
            "LEASE_ACTIVE",
            &new.project_id,
            &new.session_id,
            &new.lease_id,
            None,
            lease_event_payload(&new.project_id,&new.task_id,&new.lease_id,1,"ACTIVE")?,
            &new.issued_at,
        )?)?;

        tx.execute(
            "UPDATE tasks SET status='LEASED', updated_at=?1 WHERE task_id=?2 AND status='LEASE_REQUESTED'",
            rusqlite::params![new.issued_at, new.task_id],
        ).map_err(StorageError::Db)?;
        append_event_in(&tx, &transition_event(
            &tx,
            &format!("task_{}", new.task_id),
            "TASK_LEASED",
            &new.project_id,
            &new.session_id,
            &new.lease_id,
            None,
            task_lease_task_event_payload(&new.project_id,&new.task_id,&new.lease_id,"LEASED")?,
            &new.issued_at,
        )?)?;

        tx.commit().map_err(StorageError::Db)?;
        self.get_task_lease(&new.lease_id)?.ok_or_else(|| StorageError::NotFound(format!("task lease {}", new.lease_id)))
    }

    pub fn get_task_lease(&self, lease_id: &str) -> Result<Option<TaskLeaseRecord>> {
        self.conn.query_row(
            "SELECT lease_id,task_id,project_id,agent_id,session_id,workspace_id,lease_version,project_epoch,context_snapshot_id,state_digest,allowed_paths_json,required_capabilities_json,policy_scope,issued_at,heartbeat_at,expires_at,status FROM task_leases WHERE lease_id=?1",
            [lease_id],
            |row| Ok(TaskLeaseRecord {
                lease_id: row.get(0)?, task_id: row.get(1)?, project_id: row.get(2)?,
                agent_id: row.get(3)?, session_id: row.get(4)?, workspace_id: row.get(5)?,
                lease_version: row.get(6)?, project_epoch: row.get(7)?, context_snapshot_id: row.get(8)?,
                state_digest: row.get(9)?, allowed_paths_json: row.get(10)?, required_capabilities_json: row.get(11)?,
                policy_scope: row.get(12)?, issued_at: row.get(13)?, heartbeat_at: row.get(14)?,
                expires_at: row.get(15)?, status: row.get(16)?,
            }),
        ).optional().map_err(StorageError::Db)
    }

    /// Renew a live lease with an optimistic version check. The update is persisted as RENEWING -> ACTIVE in one
    /// transaction, so a crash cannot leave a half-incremented fencing value.
    pub fn renew_lease(
        &mut self,
        lease_id: &str,
        expected_version: i64,
        heartbeat_at: &str,
        expires_at: &str,
    ) -> Result<TaskLeaseRecord> {
        if expected_version < 1 {
            return Err(StorageError::Malformed {
                column: "task_leases.lease_version".to_string(),
                detail: "expected lease version must be positive".to_string(),
            });
        }
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        let lease: Option<(String,String,String,String,String,String,i64,i64)> = tx.query_row(
            "SELECT task_id,project_id,agent_id,session_id,workspace_id,status,lease_version,project_epoch FROM task_leases WHERE lease_id=?1",
            [lease_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)),
        ).optional().map_err(StorageError::Db)?;
        let (task_id,project_id,agent_id,session_id,workspace_id,status,current_version,project_epoch) =
            lease.ok_or_else(|| StorageError::NotFound(format!("task lease {lease_id}")))?;
        if status != "ACTIVE" {
            return Err(StorageError::Malformed {
                column: "task_leases.status".to_string(),
                detail: format!("lease {lease_id} cannot renew from {status}"),
            });
        }
        if current_version != expected_version {
            return Err(StorageError::StaleFence {
                attempt_id: lease_id.to_owned(),
                attempt_fence: current_version,
                current_lease_version: current_version,
                presented: expected_version,
            });
        }
        tx.execute(
            "UPDATE task_leases SET status='RENEWING', lease_version=lease_version+1, heartbeat_at=?1, expires_at=?2 WHERE lease_id=?3 AND status='ACTIVE' AND lease_version=?4",
            rusqlite::params![heartbeat_at,expires_at,lease_id,expected_version],
        ).map_err(StorageError::Db)?;
        append_event_in(&tx, &transition_event(
            &tx,lease_id,"LEASE_RENEWED",&project_id,&session_id,lease_id,None,
            lease_event_payload(&project_id,&task_id,lease_id,expected_version+1,"RENEWING")?,heartbeat_at
        )?)?;
        tx.execute(
            "UPDATE task_leases SET status='ACTIVE' WHERE lease_id=?1 AND status='RENEWING' AND lease_version=?2",
            rusqlite::params![lease_id,expected_version+1],
        ).map_err(StorageError::Db)?;
        append_event_in(&tx, &transition_event(
            &tx,lease_id,"LEASE_ACTIVE",&project_id,&session_id,lease_id,None,
            lease_event_payload(&project_id,&task_id,lease_id,expected_version+1,"ACTIVE")?,heartbeat_at
        )?)?;
        tx.commit().map_err(StorageError::Db)?;
        self.get_task_lease(lease_id)?.ok_or_else(|| StorageError::NotFound(format!("task lease {lease_id}")))
    }

    pub fn release_lease(&mut self, lease_id: &str, expected_version: i64, now: &str) -> Result<()> {
        self.finish_lease(lease_id, expected_version, "RELEASED", "LEASE_RELEASED", now)
    }

    pub fn revoke_lease(&mut self, lease_id: &str, expected_version: i64, now: &str) -> Result<()> {
        self.finish_lease(lease_id, expected_version, "REVOKED", "LEASE_REVOKED", now)
    }

    pub fn expire_due_leases(&mut self, now: &str) -> Result<u64> {
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        let mut stmt = tx.prepare(
            "SELECT lease_id,task_id,project_id,session_id,lease_version,status FROM task_leases
             WHERE status IN ('ACTIVE','RENEWING') AND expires_at <= ?1"
        ).map_err(StorageError::Db)?;
        let leases: Vec<(String,String,String,String,i64,String)> = stmt.query_map([now], |row|
            Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))
        ).map_err(StorageError::Db)?.collect::<std::result::Result<Vec<_>,_>>().map_err(StorageError::Db)?;
        drop(stmt);
        for (lease_id,task_id,project_id,session_id,version,status) in leases {
            let event_type = if status == "RENEWING" { "LEASE_EXPIRED" } else { "LEASE_EXPIRED" };
            tx.execute(
                "UPDATE task_leases SET status='EXPIRED' WHERE lease_id=?1 AND status=?2 AND lease_version=?3",
                rusqlite::params![lease_id,status,version],
            ).map_err(StorageError::Db)?;
            append_event_in(&tx, &transition_event(
                &tx,&lease_id,event_type,&project_id,&session_id,&lease_id,None,
                lease_event_payload(&project_id,&task_id,&lease_id,version,"EXPIRED")?,now
            )?)?;
        }
        let count = leases.len() as u64;
        tx.commit().map_err(StorageError::Db)?;
        Ok(count)
    }

    fn finish_lease(&mut self, lease_id: &str, expected_version: i64, terminal: &str, event_type: &str, now: &str) -> Result<()> {
        require_vocabulary("task_leases.status", terminal, LEASE_STATES)?;
        let tx = self.conn.transaction().map_err(StorageError::Db)?;
        let lease: Option<(String,String,String,String,String,String,i64)> = tx.query_row(
            "SELECT task_id,project_id,agent_id,session_id,workspace_id,status,lease_version FROM task_leases WHERE lease_id=?1",
            [lease_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
        ).optional().map_err(StorageError::Db)?;
        let (task_id,project_id,_agent_id,session_id,_workspace,status,current_version) =
            lease.ok_or_else(|| StorageError::NotFound(format!("task lease {lease_id}")))?;
        if !matches!(status.as_str(), "ACTIVE" | "RENEWING") {
            return Err(StorageError::Malformed {
                column: "task_leases.status".to_string(),
                detail: format!("lease {lease_id} cannot finish from {status}"),
            });
        }
        if current_version != expected_version {
            return Err(StorageError::StaleFence {
                attempt_id: lease_id.to_owned(),
                attempt_fence: current_version,
                current_lease_version: current_version,
                presented: expected_version,
            });
        }
        tx.execute(
            "UPDATE task_leases SET status=?1 WHERE lease_id=?2 AND status=?3 AND lease_version=?4",
            rusqlite::params![terminal,lease_id,status,expected_version],
        ).map_err(StorageError::Db)?;
        append_event_in(&tx, &transition_event(
            &tx,lease_id,event_type,&project_id,&session_id,lease_id,None,
            lease_event_payload(&project_id,&task_id,lease_id,current_version,terminal)?,now
        )?)?;
        tx.commit().map_err(StorageError::Db)?;
        Ok(())
    }

    /// Return READY tasks whose dependencies are COMPLETED and which have no live lease.
    ///
    /// Ordering is deterministic: higher explicit priority first, then oldest update, then oldest creation and
    /// finally task_id. The selector does not mutate state; TaskService remains the owner of lease admission.
    pub fn list_schedulable_tasks(&self, project_id: &str, limit: usize) -> Result<Vec<SchedulableTask>> {
        let limit = limit.max(1).min(256) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT t.task_id, t.project_id, t.workspace_id, t.priority, t.risk, t.created_at, t.updated_at
             FROM tasks t
             WHERE t.project_id = ?1
               AND t.status = 'READY'
               AND t.workspace_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM task_dependencies d
                   JOIN tasks dep ON dep.task_id = d.depends_on_task_id
                   WHERE d.task_id = t.task_id
                     AND dep.status <> 'COMPLETED'
               )
               AND NOT EXISTS (
                   SELECT 1 FROM task_leases l
                   WHERE l.task_id = t.task_id
                     AND l.status IN ('ACTIVE','RENEWING')
               )
             ORDER BY t.priority DESC, t.updated_at ASC, t.created_at ASC, t.task_id ASC
             LIMIT ?2"
        ).map_err(StorageError::Db)?;
        let rows = stmt.query_map(rusqlite::params![project_id, limit], |row| Ok(SchedulableTask {
            task_id: row.get(0)?,
            project_id: row.get(1)?,
            workspace_id: row.get(2)?,
            priority: row.get(3)?,
            risk: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })).map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(StorageError::Db)
    }

    /// Persist a command execution after admission. Material attempts are re-fenced against the current lease.
    pub fn insert_command_execution(&self, new: &NewCommandExecution) -> Result<()> {
        require_vocabulary("command_executions.classification", &new.classification, EXECUTION_CLASSIFICATIONS)?;
        require_vocabulary("command_executions.status", &new.status, EXECUTION_STATES)?;
        if new.timeout_seconds < 1 {
            return Err(StorageError::Malformed {
                column: "command_executions.timeout_seconds".to_string(),
                detail: "timeout_seconds must be positive".to_string(),
            });
        }
        let arguments: serde_json::Value = serde_json::from_str(&new.arguments_json).map_err(|e| StorageError::Malformed {
            column: "command_executions.arguments_json".to_string(),
            detail: format!("must be valid JSON: {e}"),
        })?;
        if !arguments.is_array() {
            return Err(StorageError::Malformed {
                column: "command_executions.arguments_json".to_string(),
                detail: "arguments_json must be a JSON array".to_string(),
            });
        }
        if new.cwd.trim().is_empty() || new.executable.trim().is_empty() {
            return Err(StorageError::Malformed {
                column: "command_executions".to_string(),
                detail: "executable and cwd must be non-empty".to_string(),
            });
        }

        if let Some(task_id) = &new.task_id {
            let matches: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE task_id = ?1 AND project_id = ?2",
                rusqlite::params![task_id, new.project_id],
                |row| row.get(0),
            ).map_err(StorageError::Db)?;
            if matches != 1 {
                return Err(StorageError::Malformed {
                    column: "command_executions.task_id".to_string(),
                    detail: format!("task {task_id} does not belong to project {}", new.project_id),
                });
            }
        }

        if let Some(attempt_id) = &new.attempt_id {
            let row: Option<(String,String,String,String,i64,String)> = self.conn.query_row(
                "SELECT project_id, task_id, workspace_id, state, fence_token, lease_id FROM task_attempts WHERE attempt_id = ?1",
                [attempt_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            ).optional().map_err(StorageError::Db)?;
            let (project_id, task_id, workspace_id, state, fence_token, _lease_id) =
                row.ok_or_else(|| StorageError::NotFound(format!("task attempt {attempt_id}")))?;
            if project_id != new.project_id
                || new.task_id.as_deref().is_some_and(|id| id != task_id)
                || workspace_id != new.workspace_id
                || !matches!(state.as_str(), "STARTED" | "RUNNING" | "CHECKPOINTED")
            {
                return Err(StorageError::Malformed {
                    column: "command_executions.attempt_id".to_string(),
                    detail: format!("attempt {attempt_id} is not an admitted execution parent"),
                });
            }
            self.verify_attempt_fence(attempt_id, fence_token)?;
        }

        if let Some(snapshot_id) = &new.environment_snapshot_id {
            let row: Option<(String,Option<String>,Option<String>,Option<String>)> = self.conn.query_row(
                "SELECT project_id, workspace_id, task_id, execution_id FROM environment_snapshots WHERE environment_snapshot_id = ?1",
                [snapshot_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).optional().map_err(StorageError::Db)?;
            let (project_id, workspace_id, task_id, execution_id) =
                row.ok_or_else(|| StorageError::NotFound(format!("environment snapshot {snapshot_id}")))?;
            if project_id != new.project_id
                || workspace_id.as_deref().is_some_and(|id| id != new.workspace_id)
                || task_id.as_deref().is_some_and(|id| Some(id) != new.task_id.as_ref())
                || execution_id.as_deref().is_some_and(|id| id != new.execution_id)
            {
                return Err(StorageError::Malformed {
                    column: "command_executions.environment_snapshot_id".to_string(),
                    detail: "environment snapshot does not match execution provenance".to_string(),
                });
            }
        }

        self.conn.execute(
            "INSERT INTO command_executions (execution_id,project_id,task_id,attempt_id,workspace_id,requested_by_agent_id,environment_snapshot_id,supersedes_binding_id,classification,executable,arguments_json,cwd,status,exit_code,started_at,ended_at,timeout_seconds,stdout_artifact_id,stderr_artifact_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            rusqlite::params![
                new.execution_id, new.project_id, new.task_id, new.attempt_id, new.workspace_id,
                new.requested_by_agent_id, new.environment_snapshot_id, new.supersedes_binding_id,
                new.classification, new.executable, new.arguments_json, new.cwd, new.status,
                new.exit_code, new.started_at, new.ended_at, new.timeout_seconds,
                new.stdout_artifact_id, new.stderr_artifact_id
            ],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    /// Read one durable command execution record.
    pub fn get_command_execution(&self, execution_id: &str) -> Result<Option<CommandExecutionRecord>> {
        self.conn.query_row(
            "SELECT execution_id,project_id,task_id,attempt_id,workspace_id,requested_by_agent_id,environment_snapshot_id,supersedes_binding_id,classification,executable,arguments_json,cwd,status,exit_code,started_at,ended_at,timeout_seconds,stdout_artifact_id,stderr_artifact_id FROM command_executions WHERE execution_id = ?1",
            [execution_id],
            |row| Ok(CommandExecutionRecord {
                execution_id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                attempt_id: row.get(3)?,
                workspace_id: row.get(4)?,
                requested_by_agent_id: row.get(5)?,
                environment_snapshot_id: row.get(6)?,
                supersedes_binding_id: row.get(7)?,
                classification: row.get(8)?,
                executable: row.get(9)?,
                arguments_json: row.get(10)?,
                cwd: row.get(11)?,
                status: row.get(12)?,
                exit_code: row.get(13)?,
                started_at: row.get(14)?,
                ended_at: row.get(15)?,
                timeout_seconds: row.get(16)?,
                stdout_artifact_id: row.get(17)?,
                stderr_artifact_id: row.get(18)?,
            }),
        ).optional().map_err(StorageError::Db)
    }

    /// Compare-and-swap one command-execution state. Material terminal state is never inferred from exit code alone.
    pub fn transition_command_execution(
        &self,
        execution_id: &str,
        expected_state: &str,
        next_state: &str,
        exit_code: Option<i64>,
        ended_at: Option<&str>,
    ) -> Result<()> {
        require_vocabulary("command_executions.expected_state", expected_state, EXECUTION_STATES)?;
        require_vocabulary("command_executions.next_state", next_state, EXECUTION_STATES)?;
        let legal = matches!(
            (expected_state, next_state),
            ("REQUESTED","POLICY_CHECK")
            | ("POLICY_CHECK","APPROVED")
            | ("APPROVED","STARTING")
            | ("STARTING","RUNNING")
            | ("RUNNING","EXITED")
            | ("EXITED","EVIDENCE_CAPTURED")
            | ("EVIDENCE_CAPTURED","RECORDED")
            | ("POLICY_CHECK","DENIED")
            | ("RUNNING","TIMEOUT")
            | ("RUNNING","CANCELED")
            | ("RUNNING","CRASHED")
            | ("CRASHED","CLEANUP_REQUIRED")
        );
        if !legal {
            return Err(StorageError::Malformed {
                column: "command_executions.status".to_string(),
                detail: format!("illegal execution transition {expected_state} -> {next_state}"),
            });
        }
        let changed = self.conn.execute(
            "UPDATE command_executions SET status=?1, exit_code=COALESCE(?2,exit_code), ended_at=COALESCE(?3,ended_at) WHERE execution_id=?4 AND status=?5",
            rusqlite::params![next_state, exit_code, ended_at, execution_id, expected_state],
        ).map_err(StorageError::Db)?;
        if changed == 0 {
            let exists = self.conn.query_row(
                "SELECT COUNT(*) FROM command_executions WHERE execution_id=?1",
                [execution_id],
                |row| row.get::<_,i64>(0),
            ).map_err(StorageError::Db)?;
            if exists == 0 {
                return Err(StorageError::NotFound(format!("command execution {execution_id}")));
            }
            return Err(StorageError::Malformed {
                column: "command_executions.status".to_string(),
                detail: format!("compare-and-swap failed: expected {expected_state}"),
            });
        }
        Ok(())
    }

    /// Append one physical-process observation. Observations are append-only; UNKNOWN is explicit uncertainty.
    pub fn insert_process_record(&self, new: &NewProcessRecord) -> Result<()> {
        require_vocabulary("process_records.state", &new.state, PROCESS_STATES)?;
        if new.pid <= 0 || new.parent_pid.is_some_and(|pid| pid <= 0) {
            return Err(StorageError::Malformed {
                column: "process_records.pid".to_string(),
                detail: "pid values must be positive".to_string(),
            });
        }
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM command_executions WHERE execution_id=?1",
            [new.execution_id.as_str()],
            |row| row.get(0),
        ).map_err(StorageError::Db)?;
        if exists != 1 {
            return Err(StorageError::NotFound(format!("command execution {}", new.execution_id)));
        }
        self.conn.execute(
            "INSERT INTO process_records (process_record_id,execution_id,pid,parent_pid,state,observed_at) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                new.process_record_id, new.execution_id, new.pid, new.parent_pid, new.state, new.observed_at
            ],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    /// Return process observations newest-first so reconciliation can use the latest physical fact.
    pub fn list_process_records(&self, execution_id: &str) -> Result<Vec<ProcessRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT process_record_id,execution_id,pid,parent_pid,state,observed_at
             FROM process_records WHERE execution_id=?1 ORDER BY observed_at DESC, process_record_id DESC"
        ).map_err(StorageError::Db)?;
        let rows = stmt.query_map([execution_id], |row| Ok(ProcessRecord {
            process_record_id: row.get(0)?,
            execution_id: row.get(1)?,
            pid: row.get(2)?,
            parent_pid: row.get(3)?,
            state: row.get(4)?,
            observed_at: row.get(5)?,
        })).map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(StorageError::Db)
    }

    /// The most recent process observation, if any.
    pub fn latest_process_record(&self, execution_id: &str) -> Result<Option<ProcessRecord>> {
        Ok(self.list_process_records(execution_id)?.into_iter().next())
    }

    /// Persist one task attempt. Task identity is stable; attempt number is unique per task.
    pub fn insert_task_attempt(&self, new: &NewTaskAttempt) -> Result<()> {
        require_vocabulary("task_attempts.state", &new.state, TASK_ATTEMPT_STATES)?;
        let lease: Option<(String,String,String,String,String,i64,String,i64)> = self.conn.query_row(
            "SELECT task_id, project_id, agent_id, session_id, workspace_id, project_epoch, context_snapshot_id, lease_version
             FROM task_leases
             WHERE lease_id = ?1 AND status IN ('ACTIVE','RENEWING')",
            [new.lease_id.as_str()],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
        ).optional().map_err(StorageError::Db)?;
        let (lease_task,lease_project,lease_agent,lease_session,lease_workspace,lease_epoch,lease_context,lease_version) =
            match lease {
                Some(v) => v,
                None => return Err(StorageError::NotFound(format!("active lease {}", new.lease_id))),
            };
        if lease_version != new.fence_token {
            return Err(StorageError::StaleFence {
                attempt_id: new.attempt_id.clone(),
                attempt_fence: new.fence_token,
                current_lease_version: lease_version,
                presented: new.fence_token,
            });
        }
        if lease_task != new.task_id || lease_project != new.project_id || lease_agent != new.agent_id
            || lease_session != new.session_id || lease_workspace != new.workspace_id
            || lease_epoch != new.project_epoch || lease_context != new.context_snapshot_id {
            return Err(StorageError::Malformed {
                column: "task_attempts".to_string(),
                detail: format!("attempt {} does not match the authoritative lease/task/workspace/context binding", new.attempt_id),
            });
        }
        self.conn.execute(
            "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, checkpoint_id, failure_id, started_at, heartbeat_at, ended_at, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            rusqlite::params![new.attempt_id,new.task_id,new.project_id,new.attempt_no,new.lease_id,new.agent_id,new.session_id,new.workspace_id,new.fence_token,new.project_epoch,new.context_snapshot_id,new.state,new.checkpoint_id,new.failure_id,new.started_at,new.heartbeat_at,new.ended_at,new.created_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    /// Refresh an attempt heartbeat only when its lease fence is still current.
    pub fn heartbeat_task_attempt(
        &self,
        attempt_id: &str,
        lease_version: i64,
        heartbeat_at: &str,
    ) -> Result<()> {
        self.verify_attempt_fence(attempt_id, lease_version)?;
        let changed = self.conn.execute(
            "UPDATE task_attempts
             SET heartbeat_at = ?1
             WHERE attempt_id = ?2 AND state IN ('STARTED','RUNNING','CHECKPOINTED')",
            rusqlite::params![heartbeat_at, attempt_id],
        ).map_err(StorageError::Db)?;
        if changed != 1 {
            return Err(StorageError::NotFound(format!("active task attempt {attempt_id}")));
        }
        Ok(())
    }

    /// Refuse a material operation when the task lease version no longer equals the attempt's fence value.
    pub fn transition_task_attempt(&self, attempt_id: &str, expected_state: &str, next_state: &str, ended_at: Option<&str>) -> Result<()> {
        require_vocabulary("task_attempts.next_state", next_state, TASK_ATTEMPT_STATES)?;
        let allowed = match (expected_state, next_state) {
            ("CREATED","STARTED")
            | ("STARTED","RUNNING")
            | ("STARTED","FAILED")
            | ("STARTED","CANCELLED")
            | ("STARTED","UNKNOWN")
            | ("RUNNING","CHECKPOINTED")
            | ("RUNNING","COMPLETED")
            | ("RUNNING","FAILED")
            | ("RUNNING","TIMED_OUT")
            | ("RUNNING","LOST")
            | ("RUNNING","CANCELLED")
            | ("RUNNING","UNKNOWN")
            | ("CHECKPOINTED","RUNNING")
            | ("CHECKPOINTED","COMPLETED")
            | ("CHECKPOINTED","FAILED")
            | ("CHECKPOINTED","LOST")
            | ("CHECKPOINTED","CANCELLED")
            | ("CHECKPOINTED","UNKNOWN") => true,
            _ => false,
        };
        if !allowed {
            return Err(StorageError::InvalidAttemptTransition {
                attempt_id: attempt_id.to_string(),
                from: expected_state.to_string(),
                to: next_state.to_string(),
            });
        }
        let changed = self.conn.execute(
            "UPDATE task_attempts SET state = ?1, ended_at = CASE WHEN ?1 IN ('COMPLETED','FAILED','TIMED_OUT','LOST','CANCELLED','UNKNOWN') THEN COALESCE(ended_at, ?4) ELSE ended_at END WHERE attempt_id = ?2 AND state = ?3",
            rusqlite::params![next_state, attempt_id, expected_state, ended_at],
        ).map_err(StorageError::Db)?;
        if changed != 1 {
            return Err(StorageError::InvalidAttemptTransition {
                attempt_id: attempt_id.to_string(),
                from: expected_state.to_string(),
                to: next_state.to_string(),
            });
        }
        Ok(())
    }

    /// Refuse a material operation when the task lease version no longer equals the attempt's fence value.
    pub fn verify_attempt_fence(&self, attempt_id: &str, presented_lease_version: i64) -> Result<()> {
        let row: Option<(i64, i64)> = self.conn.query_row(
            "SELECT ta.fence_token, tl.lease_version FROM task_attempts ta JOIN task_leases tl ON tl.lease_id = ta.lease_id WHERE ta.attempt_id = ?1 AND ta.state IN ('STARTED','RUNNING','CHECKPOINTED') AND tl.status IN ('ACTIVE','RENEWING')",
            [attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional().map_err(StorageError::Db)?;
        match row {
            Some((fence_token, current)) if fence_token == current && presented_lease_version == current => Ok(()),
            Some((fence_token, current)) => Err(StorageError::StaleFence { attempt_id: attempt_id.to_string(), attempt_fence: fence_token, current_lease_version: current, presented: presented_lease_version }),
            None => Err(StorageError::NotFound(format!("active lease for task attempt {attempt_id}"))),
        }
    }

    pub fn insert_resource_reservation(&self, new: &NewResourceReservation) -> Result<()> {
        require_vocabulary("resource_reservations.resource_type", &new.resource_type, RESOURCE_TYPES)?;
        require_vocabulary("resource_reservations.mode", &new.mode, RESOURCE_MODES)?;
        require_vocabulary("resource_reservations.state", &new.state, RESOURCE_STATES)?;
        let lease: Option<(String,String,String,String,String,i64)> = self.conn.query_row(
            "SELECT task_id, project_id, agent_id, session_id, workspace_id, lease_version
             FROM task_leases
             WHERE lease_id = ?1 AND status IN ('ACTIVE','RENEWING')",
            [new.lease_id.as_str()],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
        ).optional().map_err(StorageError::Db)?;
        let (lease_task,lease_project,_lease_agent,_lease_session,_lease_workspace,lease_version) =
            match lease { Some(v) => v, None => return Err(StorageError::NotFound(format!("active lease {}", new.lease_id))) };
        if lease_version != new.lease_version {
            return Err(StorageError::StaleFence {
                attempt_id: format!("reservation:{}", new.reservation_id),
                attempt_fence: new.lease_version,
                current_lease_version: lease_version,
                presented: new.lease_version,
            });
        }
        if lease_task != new.task_id || lease_project != new.project_id {
            return Err(StorageError::Malformed {
                column: "resource_reservations".to_string(),
                detail: format!("reservation {} does not match the authoritative task/project/lease binding", new.reservation_id),
            });
        }
        self.conn.execute(
            "INSERT INTO resource_reservations (reservation_id, project_id, task_id, lease_id, lease_version, resource_type, resource_key, mode, quantity, state, issued_at, expires_at, released_at, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![new.reservation_id,new.project_id,new.task_id,new.lease_id,new.lease_version,new.resource_type,new.resource_key,new.mode,new.quantity,new.state,new.issued_at,new.expires_at,new.released_at,new.created_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    pub fn insert_workspace_revision(&self, new: &NewWorkspaceRevision) -> Result<()> {
        require_vocabulary("workspace_revisions.source", &new.source, REVISION_SOURCES)?;
        require_vocabulary("workspace_revisions.status", &new.status, REVISION_STATES)?;
        self.conn.execute(
            "INSERT INTO workspace_revisions (revision_id, project_id, workspace_id, task_id, session_id, revision_no, parent_revision_id, repository_head, manifest_hash, tree_hash, source, status, observed_at, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![new.revision_id,new.project_id,new.workspace_id,new.task_id,new.session_id,new.revision_no,new.parent_revision_id,new.repository_head,new.manifest_hash,new.tree_hash,new.source,new.status,new.observed_at,new.created_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    pub fn insert_environment_snapshot(&self, new: &NewEnvironmentSnapshot) -> Result<()> {
        require_vocabulary("environment_snapshots.source", &new.source, &["PREFLIGHT", "EXECUTION", "VALIDATION"])?;
        let json_ok: i64 = self.conn.query_row(
            "SELECT CASE WHEN json_valid(?1) = 1 AND json_type(?1, '
            "INSERT INTO environment_snapshots (environment_snapshot_id, project_id, workspace_id, task_id, execution_id, os_identity, runtime_versions_json, environment_policy_hash, source, captured_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![new.environment_snapshot_id,new.project_id,new.workspace_id,new.task_id,new.execution_id,new.os_identity,new.runtime_versions_json,new.environment_policy_hash,new.source,new.captured_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    pub fn insert_certification_binding(&self, new: &NewCertificationBinding) -> Result<()> {
        require_vocabulary("certification_bindings.status", &new.status, CERTIFICATION_STATES)?;
        let validation: Option<(String, Option<String>, String)> = self.conn.query_row(
            "SELECT project_id, task_id, verdict FROM validation_runs WHERE validation_id = ?1",
            [new.validation_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional().map_err(StorageError::Db)?;
        let (validation_project, validation_task, validation_verdict) = match validation {
            Some(v) => v,
            None => return Err(StorageError::NotFound(format!("validation run {}", new.validation_id))),
        };
        if validation_project != new.project_id {
            return Err(StorageError::Malformed {
                column: "certification_bindings.project_id".to_string(),
                detail: format!("validation {} belongs to project {validation_project}, not {}", new.validation_id, new.project_id),
            });
        }
        if new.task_id != validation_task {
            return Err(StorageError::Malformed {
                column: "certification_bindings.task_id".to_string(),
                detail: format!("certification task scope does not match validation {} task scope", new.validation_id),
            });
        }
        if new.status == "ASSERTED" && validation_verdict != "PASS" {
            return Err(StorageError::Malformed {
                column: "validation_runs.verdict".to_string(),
                detail: format!("validation {} has verdict {validation_verdict}; ASSERTED certification requires PASS", new.validation_id),
            });
        }
        if let Some(parent_id) = new.supersedes_binding_id.as_deref() {
            let parent: Option<(String, Option<String>)> = self.conn.query_row(
                "SELECT project_id, task_id FROM certification_bindings WHERE certification_binding_id = ?1",
                [parent_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional().map_err(StorageError::Db)?;
            match parent {
                None => return Err(StorageError::NotFound(format!("certification binding {parent_id}"))),
                Some((project, task)) if project != new.project_id || task != new.task_id => {
                    return Err(StorageError::Malformed {
                        column: "certification_bindings.supersedes_binding_id".to_string(),
                        detail: format!("binding {parent_id} does not match the new certification project/task scope"),
                    })
                }
                Some(_) => {}
            }
        }
        let valid: i64 = self.conn.query_row(
            "SELECT CASE WHEN json_valid(?1) = 1 AND json_type(?1, '
        self.conn.execute(
            "INSERT INTO certification_bindings (certification_binding_id, project_id, task_id, validation_id, workspace_revision_id, environment_snapshot_id, artifact_hashes_json, validator_version, test_suite_version, status, supersedes_binding_id, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![new.certification_binding_id,new.project_id,new.task_id,new.validation_id,new.workspace_revision_id,new.environment_snapshot_id,new.artifact_hashes_json,new.validator_version,new.test_suite_version,new.status,new.supersedes_binding_id,new.reason,new.created_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }
}


pub struct RecoverableAttempt {
    pub attempt_id: String,
    pub task_id: String,
    pub attempt_no: i64,
    pub state: String,
    pub lease_id: String,
    pub fence_token: i64,
    pub current_lease_version: Option<i64>,
    pub lease_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableExecution {
    pub execution_id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub status: String,
    pub pid: Option<i64>,
    pub process_state: Option<String>,
}

pub struct NewTraceLink {
    pub trace_link_id: String,
    pub project_id: String,
    pub link_type: String,
    pub source_type: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub created_at: String,
}

pub struct TraceLinkRecord {
    pub trace_link_id: String,
    pub project_id: String,
    pub link_type: String,
    pub source_type: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub created_at: String,
}

const TRACE_LINK_TYPES: &[&str] = &[
    "INTENT_REQUIREMENT","REQUIREMENT_ACCEPTANCE","REQUIREMENT_DECISION",
    "DECISION_ARCHITECTURE","ARCHITECTURE_CONTRACT","CONTRACT_TASK",
    "TASK_ATTEMPT","TASK_LEASE","ATTEMPT_CHECKPOINT","ATTEMPT_EXECUTION",
    "LEASE_CHANGESET","CHANGESET_EXECUTION","EXECUTION_ENVIRONMENT",
    "EXECUTION_EVIDENCE","EVIDENCE_REVIEW","REVIEW_VALIDATION",
    "VALIDATION_ENVIRONMENT","VALIDATION_CERTIFICATION",
];



impl Storage {

    /// Transition a held resource reservation to RELEASED using the current lease fence.
    pub fn release_resource_reservation(
        &self,
        reservation_id: &str,
        lease_version: i64,
        released_at: &str,
    ) -> Result<()> {
        let row: Option<(String, String, i64, i64)> = self.conn.query_row(
            "SELECT rr.resource_type, rr.resource_key, rr.lease_version, tl.lease_version
             FROM resource_reservations rr
             JOIN task_leases tl ON tl.lease_id = rr.lease_id
             WHERE rr.reservation_id = ?1 AND rr.state = 'HELD' AND tl.status IN ('ACTIVE','RENEWING')",
            [reservation_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional().map_err(StorageError::Db)?;
        let (resource_type, resource_key, stored_version, current_version) = match row {
            Some(v) => v,
            None => return Err(StorageError::NotFound(format!("held resource reservation {reservation_id}"))),
        };
        if stored_version != current_version || current_version != lease_version {
            return Err(StorageError::StaleFence {
                attempt_id: format!("reservation:{reservation_id}"),
                attempt_fence: stored_version,
                current_lease_version: lease_version,
                presented: lease_version,
            });
        }
        let changed = self.conn.execute(
            "UPDATE resource_reservations
             SET state = 'RELEASED', released_at = ?1
             WHERE reservation_id = ?2 AND state = 'HELD' AND lease_version = ?3",
            rusqlite::params![released_at, reservation_id, lease_version],
        ).map_err(StorageError::Db)?;
        if changed != 1 {
            return Err(StorageError::Malformed {
                column: "resource_reservations.state".to_string(),
                detail: format!("reservation {reservation_id} changed while being released ({resource_type}:{resource_key})"),
            });
        }
        Ok(())
    }

    /// Expire reservations whose durable deadline has passed. This is a recovery operation, not a lease-authorized mutation.
    pub fn expire_due_resource_reservations(&self, now: &str) -> Result<u64> {
        let changed = self.conn.execute(
            "UPDATE resource_reservations
             SET state = 'EXPIRED'
             WHERE state = 'HELD' AND expires_at <= ?1",
            [now],
        ).map_err(StorageError::Db)?;
        Ok(changed as u64)
    }

    /// List attempts whose physical execution may still need reconciliation after a restart.
    pub fn list_recoverable_attempts(&self, project_id: &str) -> Result<Vec<RecoverableAttempt>> {
        let mut stmt = self.conn.prepare(
            "SELECT ta.attempt_id, ta.task_id, ta.attempt_no, ta.state, ta.lease_id, ta.fence_token,
                    tl.lease_version, tl.status
             FROM task_attempts ta
             LEFT JOIN task_leases tl ON tl.lease_id = ta.lease_id
             WHERE ta.project_id = ?1 AND ta.state IN ('STARTED','RUNNING','CHECKPOINTED','UNKNOWN')
             ORDER BY ta.attempt_no ASC, ta.attempt_id ASC"
        ).map_err(StorageError::Db)?;
        let rows = stmt.query_map([project_id], |r| Ok(RecoverableAttempt {
            attempt_id:r.get(0)?, task_id:r.get(1)?, attempt_no:r.get(2)?,
            state:r.get(3)?, lease_id:r.get(4)?, fence_token:r.get(5)?,
            current_lease_version:r.get(6)?, lease_status:r.get(7)?
        })).map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(StorageError::Db)
    }

    /// List nonterminal or cleanup-relevant executions and their newest physical process observation.
    pub fn list_recoverable_executions(&self, project_id: &str) -> Result<Vec<RecoverableExecution>> {
        let mut stmt = self.conn.prepare(
            "SELECT ce.execution_id, ce.project_id, ce.task_id, ce.attempt_id, ce.status,
                    pr.pid, pr.state
             FROM command_executions ce
             LEFT JOIN process_records pr
               ON pr.process_record_id = (
                   SELECT p2.process_record_id
                   FROM process_records p2
                   WHERE p2.execution_id = ce.execution_id
                   ORDER BY p2.observed_at DESC, p2.process_record_id DESC
                   LIMIT 1
               )
             WHERE ce.project_id = ?1
               AND ce.status IN ('STARTING','RUNNING','TIMEOUT','CANCELED','CRASHED','CLEANUP_REQUIRED')
             ORDER BY ce.execution_id ASC"
        ).map_err(StorageError::Db)?;
        let rows = stmt.query_map([project_id], |r| Ok(RecoverableExecution {
            execution_id:r.get(0)?, project_id:r.get(1)?, task_id:r.get(2)?,
            attempt_id:r.get(3)?, status:r.get(4)?, pid:r.get(5)?, process_state:r.get(6)?
        })).map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(StorageError::Db)
    }

    /// Resolve the effective latest certification claim without mutating historical rows.
    pub fn get_latest_certification_binding(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> Result<Option<CertificationBindingRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.certification_binding_id, b.project_id, b.task_id, b.validation_id,
                    b.workspace_revision_id, b.environment_snapshot_id, b.artifact_hashes_json,
                    b.validator_version, b.test_suite_version, b.status, b.supersedes_binding_id,
                    b.reason, b.created_at
             FROM certification_bindings b
             WHERE b.project_id = ?1
               AND (?2 IS NULL AND b.task_id IS NULL OR ?2 IS NOT NULL AND b.task_id = ?2)
               AND NOT EXISTS (
                   SELECT 1 FROM certification_bindings newer
                   WHERE newer.supersedes_binding_id = b.certification_binding_id
               )
             ORDER BY b.created_at DESC, b.certification_binding_id DESC
             LIMIT 1",
        ).map_err(StorageError::Db)?;
        stmt.query_row(
            rusqlite::params![project_id, task_id],
            |r| Ok(CertificationBindingRecord {
                certification_binding_id:r.get(0)?, project_id:r.get(1)?, task_id:r.get(2)?,
                validation_id:r.get(3)?, workspace_revision_id:r.get(4)?,
                environment_snapshot_id:r.get(5)?, artifact_hashes_json:r.get(6)?,
                validator_version:r.get(7)?, test_suite_version:r.get(8)?,
                status:r.get(9)?, supersedes_binding_id:r.get(10)?,
                reason:r.get(11)?, created_at:r.get(12)?
            }),
        ).optional().map_err(StorageError::Db)
    }

    pub fn insert_trace_link(&self, new: &NewTraceLink) -> Result<()> {
        require_vocabulary("trace_links.link_type", &new.link_type, TRACE_LINK_TYPES)?;
        if new.source_type.trim().is_empty() || new.source_id.trim().is_empty()
            || new.target_type.trim().is_empty() || new.target_id.trim().is_empty() {
            return Err(StorageError::Malformed {
                column: "trace_links".to_string(),
                detail: "source/target type and id must be non-empty".to_string(),
            });
        }
        self.conn.execute(
            "INSERT INTO trace_links (trace_link_id, project_id, link_type, source_type, source_id, target_type, target_id, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                new.trace_link_id,new.project_id,new.link_type,new.source_type,
                new.source_id,new.target_type,new.target_id,new.created_at
            ],
        ).map_err(StorageError::Db)?;
        Ok(())
    }

    pub fn list_trace_links(&self, project_id: &str) -> Result<Vec<TraceLinkRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT trace_link_id, project_id, link_type, source_type, source_id, target_type, target_id, created_at
             FROM trace_links WHERE project_id = ?1
             ORDER BY created_at ASC, trace_link_id ASC"
        ).map_err(StorageError::Db)?;
        let rows = stmt.query_map([project_id], |r| Ok(TraceLinkRecord {
            trace_link_id:r.get(0)?, project_id:r.get(1)?, link_type:r.get(2)?,
            source_type:r.get(3)?, source_id:r.get(4)?, target_type:r.get(5)?,
            target_id:r.get(6)?, created_at:r.get(7)?
        })).map_err(StorageError::Db)?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(StorageError::Db)
    }
}
) = 'object' THEN 1 ELSE 0 END",
            [new.runtime_versions_json.as_str()],
            |r| r.get(0),
        ).map_err(StorageError::Db)?;
        if json_ok != 1 {
            return Err(StorageError::MalformedJson {
                column: "environment_snapshots.runtime_versions_json".to_string(),
            });
        }
        self.conn.execute(
            "INSERT INTO environment_snapshots (environment_snapshot_id, project_id, workspace_id, task_id, execution_id, os_identity, runtime_versions_json, environment_policy_hash, source, captured_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![new.environment_snapshot_id,new.project_id,new.workspace_id,new.task_id,new.execution_id,new.os_identity,new.runtime_versions_json,new.environment_policy_hash,new.source,new.captured_at],
        ).map_err(StorageError::Db)?;
        Ok(())
    }


}


#[derive(Debug, Clone, PartialEq, Eq)]

/// Durable recovery candidate for a non-terminal task attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
