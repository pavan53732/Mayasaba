//! DiagnosticsService: read-only observations over the durable communication record.
//!
//! Every value here is read or derived from `SQLite`; none is inferred, cached or invented. Two derivations are
//! worth naming because they are the places a plausible-looking answer could have been fabricated instead:
//!
//! - **Open gaps are derived, never stored.** `crates/bus` records the same decision: the positions are already
//!   durable in `messages`, so "a position is missing?" is a question about the record rather than a second fact
//!   recorded beside it, and a second record of one fact is a second fact that can disagree.
//! - **The transport status is a constant, not a probe.** No agent transport exists until M3, so the honest
//!   answer is `not_connected` rather than a status that implies something was contacted (DEC-073).
//!
//! Nothing in this module writes. `DiagnosticsService` takes `&Bus` and `&Storage` precisely so that it cannot.

use mayasaba_bus::{Bus, BusError};
use mayasaba_storage::{Storage, StorageError};

/// The registry code every derived gap carries.
///
/// `SEQUENCE_GAP` is registered in `schemas/error-v1/registry.json` as category `SEQUENCE`, retryability
/// `AFTER_SYNC`, severity `WARNING`. The derived gap reuses it rather than inventing a diagnostics-only code
/// (DEC-055): the condition is the one the registry already describes.
pub const SEQUENCE_GAP_CODE: &str = "SEQUENCE_GAP";

/// The transport status while no agent transport exists (DEC-073).
pub const TRANSPORT_NOT_CONNECTED: &str = "not_connected";

/// A position that is missing between two observed positions in one `(session_id, channel)` stream.
///
/// `expected` is the absent position and `found` is the observed position that revealed it - the next position
/// actually present after the hole. Both are derived from `messages`, which carries
/// `UNIQUE(session_id, channel, sequence)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenGap {
    /// Always [`SEQUENCE_GAP_CODE`], carried in the value so the wire needs no second lookup.
    pub code: &'static str,
    pub session_id: String,
    pub channel: String,
    pub expected: i64,
    pub found: i64,
}

/// How many messages of one project sit in each state that means "not finished".
///
/// Terminal and successful states are deliberately not counted here: this is a health view, and a count of
/// everything would answer a different question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryCounts {
    /// Queued and waiting for a transport that does not exist yet.
    pub queued: i64,
    /// Refused by the transport and waiting for another attempt.
    pub retrying: i64,
    /// Past its retry budget.
    pub expired: i64,
    /// Recorded in `dead_letters`, which is the durable dead-letter record.
    pub dead_lettered: i64,
}

/// The communication health of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationHealth {
    pub project_id: String,
    /// Outstanding dispatch work, **across all projects**, because the capacity bound is global (DEC-068).
    pub backlog: i64,
    /// The bound `backlog` is measured against, also global: `dispatch.max_pending`.
    pub enqueue_limit: i64,
    pub counts: DeliveryCounts,
    pub open_gaps: Vec<OpenGap>,
    pub transport_status: &'static str,
}

/// A consumer's durable position in a project's event stream.
///
/// The three optional fields are reported as absent rather than derived. `event_cursors` stores only
/// `last_sequence`; `next_sequence`, `gap_detected` and `resync_from` have no durable source, and no contract in
/// this repository defines how they should be computed. Inventing a derivation would put a number on the wire
/// that nothing durable supports, so they are reported empty and the gap is surfaced here instead (AGENTS.md
/// section 22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCursor {
    pub project_id: String,
    pub consumer_id: String,
    /// The durable position, or `0` when no cursor row exists.
    ///
    /// Zero means "nothing consumed", which is what the absence of a row truthfully implies. The canonical
    /// cursor schema (`schemas/ui-events-v1/cursor.schema.json`) sets `additionalProperties: false` and the
    /// error registry has no `NOT_FOUND` code, so there is no in-band way to distinguish "no row" from "a row
    /// at zero" - a limitation recorded rather than hidden.
    pub last_sequence: i64,
    pub next_sequence: Option<i64>,
    pub gap_detected: bool,
    pub resync_from: Option<i64>,
}

/// Read-only diagnostics over the durable communication record.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiagnosticsService;

impl DiagnosticsService {
    /// The communication health of one project.
    pub fn communication_health(
        bus: &Bus,
        project_id: &str,
    ) -> Result<CommunicationHealth, BusError> {
        let storage = bus.storage();
        let policy = bus.dispatch_policy();

        Ok(CommunicationHealth {
            project_id: project_id.to_string(),
            backlog: bus.backlog()?,
            enqueue_limit: policy.max_pending,
            counts: Self::delivery_counts(storage, project_id)?,
            open_gaps: Self::open_gaps(storage, project_id)?,
            transport_status: TRANSPORT_NOT_CONNECTED,
        })
    }

    /// How many messages of one project sit in each unfinished state.
    pub fn delivery_counts(
        storage: &Storage,
        project_id: &str,
    ) -> Result<DeliveryCounts, BusError> {
        Ok(DeliveryCounts {
            queued: storage
                .message_count_in_state(project_id, "QUEUED")
                .map_err(storage_failure)?,
            retrying: storage
                .message_count_in_state(project_id, "RETRYING")
                .map_err(storage_failure)?,
            expired: storage
                .message_count_in_state(project_id, "EXPIRED")
                .map_err(storage_failure)?,
            dead_lettered: storage
                .dead_letter_count(project_id)
                .map_err(storage_failure)?,
        })
    }

    /// The positions missing between observed positions in each of a project's streams.
    ///
    /// A stream's holes are derived from its own durable positions, so this needs no column and no table. The
    /// walk starts at the stream's **lowest** observed position, matching `crates/bus`: a first message has
    /// nothing to be missing from, so positions below the first arrival are not reported as gaps.
    pub fn open_gaps(storage: &Storage, project_id: &str) -> Result<Vec<OpenGap>, BusError> {
        let rows = storage
            .open_sequence_gaps(project_id)
            .map_err(storage_failure)?;
        Ok(rows
            .into_iter()
            .map(|row| OpenGap {
                code: SEQUENCE_GAP_CODE,
                session_id: row.session_id,
                channel: row.channel,
                expected: row.expected,
                found: row.found,
            })
            .collect())
    }
    /// A consumer's durable position in a project's event stream.
    pub fn event_cursor(
        storage: &Storage,
        project_id: &str,
        consumer_id: &str,
    ) -> Result<EventCursor, BusError> {
        let last_sequence = storage
            .event_cursor_sequence(project_id, consumer_id)
            .map_err(storage_failure)?;

        Ok(EventCursor {
            project_id: project_id.to_string(),
            consumer_id: consumer_id.to_string(),
            last_sequence: last_sequence.unwrap_or(0),
            next_sequence: None,
            gap_detected: false,
            resync_from: None,
        })
    }
}

/// A read failure is a storage failure and nothing more specific.
///
/// These queries carry no sequence semantics, so the bus's `classify` would have nothing to distinguish; naming
/// the variant directly keeps the error the one that actually happened.
fn storage_failure(error: StorageError) -> BusError {
    BusError::Storage(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mayasaba_storage::{NewProject, Storage};

    fn store(tag: &str) -> Storage {
        let mut storage = Storage::open_in_memory().expect("in-memory store");
        storage
            .create_project(&NewProject {
                project_id: format!("prj_{tag}"),
                local_path: format!("C:/tmp/{tag}"),
                brief_id: format!("brf_{tag}"),
                brief_body: "Body".to_string(),
                brief_source: "TEST".to_string(),
                event_id: format!("evt_{tag}_genesis"),
                created_at: "2026-10-04T00:00:00Z".to_string(),
            })
            .expect("project");
        storage
    }

    fn message(
        storage: &Storage,
        project: &str,
        session: &str,
        channel: &str,
        sequence: i64,
        state: &str,
    ) {
        storage
            .conn()
            .execute(
                "INSERT INTO messages (message_id, event_id, project_id, session_id, message_type, channel,
                                       sequence, correlation_id, causation_id, idempotency_key, delivery_state,
                                       envelope_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'TASK_PROGRESS', ?5, ?6, ?7, NULL, NULL, ?8, '{}', '2026-10-04T00:00:00Z')",
                rusqlite::params![
                    format!("{project}_{session}_{channel}_{sequence}"),
                    format!("evt_{project}_{session}_{channel}_{sequence}"),
                    project,
                    session,
                    channel,
                    sequence,
                    format!("cor_{sequence}"),
                    state,
                ],
            )
            .expect("message");
    }

    #[test]
    fn a_project_with_no_messages_has_empty_health() {
        // The read does not validate that the project exists. No registry code expresses "unknown project", and
        // adding one to reject a read that changed nothing would be a new code for a condition the vocabulary
        // does not name. The consequence is recorded rather than hidden: an unknown project reports zeros, which
        // are the counts its absence of rows actually produces.
        let bus = Bus::new(Storage::open_in_memory().expect("bus store"));
        let health = DiagnosticsService::communication_health(&bus, "prj_quiet").expect("health");

        assert_eq!(health.backlog, 0);
        assert_eq!(health.enqueue_limit, 1024);
        assert_eq!(
            health.counts,
            DeliveryCounts {
                queued: 0,
                retrying: 0,
                expired: 0,
                dead_lettered: 0
            }
        );
        assert!(health.open_gaps.is_empty());
        assert_eq!(health.transport_status, "not_connected");
    }

    #[test]
    fn counts_are_per_project_and_only_cover_unfinished_states() {
        let mut storage = store("counts");
        for (sequence, state) in [
            (1, "QUEUED"),
            (2, "QUEUED"),
            (3, "RETRYING"),
            (4, "EXPIRED"),
            (5, "DISPATCHED"),
        ] {
            message(&storage, "prj_counts", "ses_a", "TASK", sequence, state);
        }
        // A second project's messages must not leak into the first project's counts. Counts are scoped to a
        // project, not to a channel: the OTHER-channel row above belongs to prj_counts and is counted.
        storage
            .create_project(&NewProject {
                project_id: "prj_other".to_string(),
                local_path: "C:/tmp/other".to_string(),
                brief_id: "brf_other".to_string(),
                brief_body: "Body".to_string(),
                brief_source: "TEST".to_string(),
                event_id: "evt_other_genesis".to_string(),
                created_at: "2026-10-04T00:00:00Z".to_string(),
            })
            .expect("second project");
        // A distinct session id, because `messages` declares UNIQUE(session_id, channel, sequence) with no
        // project_id in the key: a session id is globally unique, not per-project.
        message(&storage, "prj_other", "ses_other", "TASK", 1, "EXPIRED");
        storage
            .conn()
            .execute(
                "INSERT INTO dead_letters (dead_letter_id, message_id, project_id, final_error_json, attempts, created_at)
                 VALUES ('dl_1', 'msg_gone', 'prj_counts', '{}', 5, '2026-10-04T00:00:00Z')",
                [],
            )
            .expect("dead letter");

        let counts = DiagnosticsService::delivery_counts(&storage, "prj_counts").expect("counts");
        assert_eq!(counts.queued, 2);
        assert_eq!(counts.retrying, 1);
        assert_eq!(counts.expired, 1);
        assert_eq!(counts.dead_lettered, 1);
    }

    #[test]
    fn a_hole_between_observed_positions_is_derived_with_its_revealing_arrival() {
        let storage = store("gaps");
        message(&storage, "prj_gaps", "ses_a", "TASK", 1, "QUEUED");
        message(&storage, "prj_gaps", "ses_a", "TASK", 4, "QUEUED");

        let gaps = DiagnosticsService::open_gaps(&storage, "prj_gaps").expect("gaps");
        assert_eq!(
            gaps,
            vec![
                OpenGap {
                    code: "SEQUENCE_GAP",
                    session_id: "ses_a".into(),
                    channel: "TASK".into(),
                    expected: 2,
                    found: 4
                },
                OpenGap {
                    code: "SEQUENCE_GAP",
                    session_id: "ses_a".into(),
                    channel: "TASK".into(),
                    expected: 3,
                    found: 4
                },
            ]
        );
    }

    #[test]
    fn a_stream_with_no_holes_reports_no_gaps() {
        let storage = store("contiguous");
        for sequence in 1..=4 {
            message(
                &storage,
                "prj_contiguous",
                "ses_a",
                "TASK",
                sequence,
                "QUEUED",
            );
        }
        assert!(DiagnosticsService::open_gaps(&storage, "prj_contiguous")
            .expect("gaps")
            .is_empty());
    }

    #[test]
    fn positions_below_the_first_arrival_are_not_gaps() {
        // The bus reports no gap for a first message, because a first message has nothing to be missing from.
        // The derivation has to agree, or diagnostics would invent gaps the bus never reported.
        let storage = store("late_start");
        message(&storage, "prj_late_start", "ses_a", "TASK", 5, "QUEUED");
        assert!(DiagnosticsService::open_gaps(&storage, "prj_late_start")
            .expect("gaps")
            .is_empty());
    }

    #[test]
    fn gaps_are_scoped_to_their_own_stream() {
        let storage = store("streams");
        message(&storage, "prj_streams", "ses_a", "TASK", 1, "QUEUED");
        message(&storage, "prj_streams", "ses_a", "TASK", 3, "QUEUED");
        // A different channel's position 2 must not fill the hole in TASK, and vice versa.
        message(&storage, "prj_streams", "ses_a", "HANDOFF", 1, "QUEUED");
        message(&storage, "prj_streams", "ses_a", "HANDOFF", 2, "QUEUED");

        let gaps = DiagnosticsService::open_gaps(&storage, "prj_streams").expect("gaps");
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].channel, "TASK");
        assert_eq!(gaps[0].expected, 2);
        assert_eq!(gaps[0].found, 3);
    }

    #[test]
    fn an_absent_cursor_is_reported_as_zero_rather_than_invented() {
        let storage = store("cursor");
        let cursor =
            DiagnosticsService::event_cursor(&storage, "prj_cursor", "ui").expect("cursor");

        assert_eq!(cursor.project_id, "prj_cursor");
        assert_eq!(cursor.consumer_id, "ui");
        assert_eq!(cursor.last_sequence, 0, "no row means nothing consumed");
        assert_eq!(cursor.next_sequence, None);
        assert!(!cursor.gap_detected);
        assert_eq!(cursor.resync_from, None);
    }

    #[test]
    fn a_stored_cursor_is_read_back_unchanged() {
        let storage = store("cursor_stored");
        storage
            .conn()
            .execute(
                "INSERT INTO event_cursors (cursor_id, project_id, consumer_id, last_sequence, updated_at)
                 VALUES ('cur_1', 'prj_cursor_stored', 'ui', 42, '2026-10-04T00:00:00Z')",
                [],
            )
            .expect("cursor row");

        let cursor =
            DiagnosticsService::event_cursor(&storage, "prj_cursor_stored", "ui").expect("cursor");
        assert_eq!(cursor.last_sequence, 42);

        // A different consumer of the same project keeps its own position.
        let other = DiagnosticsService::event_cursor(&storage, "prj_cursor_stored", "other")
            .expect("cursor");
        assert_eq!(other.last_sequence, 0);
    }

    #[test]
    fn health_reads_the_bus_capacity_bound_rather_than_a_copy_of_it() {
        // The limit is the bus's configured policy, so diagnostics cannot drift from the bound the bus
        // actually enforces by holding its own constant.
        use mayasaba_bus::{BackoffPolicy, DispatchPolicy};
        let storage = Storage::open_in_memory().expect("store");
        let bus = Bus::with_policy(
            storage,
            DispatchPolicy {
                max_pending: 7,
                ..DispatchPolicy::default()
            },
            BackoffPolicy::default(),
        );
        let health = DiagnosticsService::communication_health(&bus, "prj_any").expect("health");
        assert_eq!(health.enqueue_limit, 7);
    }

    #[test]
    fn the_diagnostic_queries_write_nothing() {
        // "Read-only" is a claim about what the queries do, so it is measured rather than asserted. SQLite
        // counts the rows this connection has modified since it opened, so an unchanged count across both
        // queries is direct evidence that neither wrote.
        let storage = store("read_only");
        message(&storage, "prj_read_only", "ses_a", "TASK", 1, "QUEUED");
        message(&storage, "prj_read_only", "ses_a", "TASK", 3, "QUEUED");
        let bus = Bus::new(storage);

        let before = bus.storage().conn().total_changes();
        let health =
            DiagnosticsService::communication_health(&bus, "prj_read_only").expect("health");
        let cursor =
            DiagnosticsService::event_cursor(bus.storage(), "prj_read_only", "ui").expect("cursor");
        let after = bus.storage().conn().total_changes();

        // The queries returned real work, so an unchanged count is not an unchanged code path.
        assert_eq!(health.counts.queued, 2);
        assert_eq!(health.open_gaps.len(), 1);
        assert_eq!(cursor.last_sequence, 0);
        assert_eq!(before, after, "a diagnostic query modified durable state");
    }
}
