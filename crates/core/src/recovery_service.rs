//! Recovery operations that are more than a store call.
//!
//! `replay_dead_letter` is the first. It re-enqueues a terminal message as a new one through the bus, but only
//! after refusing the message types whose replay would need an authorization judgement this build cannot make.
//! The refusal is the reason the module exists: without it, replay would be a path around the authorization that
//! every other material mutation has to pass (AGENTS.md section 12), and a path around a gate is a gate that is
//! not there.
//!
//! The judgement is made **before** the bus is asked for a replay. `Bus::replay` rewrites the envelope as it
//! copies it, so a decision taken afterwards would be a decision about a replay that had already happened.

use mayasaba_bus::{Bus, BusError, Clock, IdSource};
use mayasaba_protocol::generated::envelope::MATERIAL_ACTION_MESSAGE_TYPES;
use serde::Serialize;

/// Why a replay did not happen.
///
/// No variant carries a registry code. The code is chosen where the refusal becomes a wire response, so the
/// registry stays the one place a code's meaning is decided and the emission site stays visible to the gate that
/// checks every emitted code is registered.
#[derive(Debug)]
pub enum ReplayRefusal {
    /// The message is a material action, and replaying one would authorize a material action.
    ///
    /// The authorization judgement does not exist yet, so this fails closed. It is deliberately not reported as
    /// an unauthorized actor or as a policy denial: no rule ran and nothing established that the actor is not
    /// allowed, so either would put a judgement on the wire that was never made.
    AuthorizationNotImplemented {
        /// The message that was refused.
        message_id: String,
        /// Its type, so the caller can see what was refused rather than only that something was.
        message_type: String,
    },
    /// The durable record does not hold the message the request named, or does not hold it readably.
    ///
    /// This does not decide what a missing message means. The bus already decided that: its classifier maps a
    /// named row that does not exist to the schema-invalid condition rather than to a storage failure, because
    /// "nothing went wrong with the database, and reporting it as one would send a caller looking for a broken
    /// disk". A missing message is that same condition reached from the other side, and the shell applies the
    /// code the repository already chose for it.
    Unreadable {
        /// The message that was named.
        message_id: String,
        /// What the store said.
        detail: String,
    },
    /// The bus refused the replay.
    Bus(BusError),
}

/// What a completed replay produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ReplayOutcome {
    /// The message the bus produced, or the one an earlier identical replay already produced.
    pub replayed_message_id: String,
    /// The terminal message it was replayed from.
    pub source_message_id: String,
    /// The context the original was decided in, carried forward unchanged.
    pub context_snapshot_id: Option<String>,
    /// The state digest the original was decided against, carried forward unchanged.
    pub state_digest: Option<String>,
    /// Always false.
    ///
    /// A replay re-enqueues the original envelope, so the context it carries is the original's. Reporting `true`
    /// would claim a refresh that did not happen, and a caller that trusted it would act on a stale context
    /// believing it current.
    pub context_refreshed: bool,
    /// True when an earlier replay of the same operation was found and this call produced no new message.
    ///
    /// The bus reports this rather than hiding it, because "this was already queued" and "this is now queued"
    /// are different answers and a caller that cannot tell them apart cannot tell an absorbed replay from a
    /// second delivery. Surfacing it is what makes a second call observably idempotent rather than claimed to be.
    pub deduplicated: bool,
}

/// Recovery application service, owned by `core` in `workspace.manifest.json`.
pub struct RecoveryService;

impl RecoveryService {
    /// Re-enqueue a terminal message as a new one, refusing the types whose replay needs an authorizer.
    ///
    /// The terminal-state precondition is **not** restated here. The bus replays `EXPIRED` and `DEAD_LETTER` and
    /// classifies anything else, so a second copy of that rule would be a second source of truth for one
    /// condition. It is also deliberately not narrowed to the literal `DEAD_LETTER` state: the dead-letter store
    /// holds `EXPIRED` messages, because that is the state an exhausted retry budget produces (DEC-061), so
    /// requiring `DEAD_LETTER` would refuse the store's own contents.
    pub fn replay_dead_letter(
        bus: &mut Bus,
        ids: &dyn IdSource,
        clock: &dyn Clock,
        message_id: &str,
    ) -> Result<ReplayOutcome, ReplayRefusal> {
        // One read, before anything is written, carrying every fact the decision and the response need.
        let facts =
            bus.storage()
                .replay_facts(message_id)
                .map_err(|error| ReplayRefusal::Unreadable {
                    message_id: message_id.to_string(),
                    detail: error.to_string(),
                })?;

        if MATERIAL_ACTION_MESSAGE_TYPES.contains(&facts.message_type.as_str()) {
            return Err(ReplayRefusal::AuthorizationNotImplemented {
                message_id: message_id.to_string(),
                message_type: facts.message_type,
            });
        }

        let enqueued = bus
            .replay(message_id, ids, clock)
            .map_err(ReplayRefusal::Bus)?;

        Ok(ReplayOutcome {
            replayed_message_id: enqueued.message_id,
            source_message_id: message_id.to_string(),
            context_snapshot_id: facts.context_snapshot_id,
            state_digest: facts.state_digest,
            context_refreshed: false,
            deduplicated: enqueued.deduplicated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_runtime::{CoreClock, CoreIdSource};
    use mayasaba_storage::{NewProject, Storage};

    fn storage_with_project(tag: &str) -> Storage {
        let mut storage = Storage::open_in_memory().expect("store");
        storage
            .create_project(&NewProject {
                project_id: format!("prj_{tag}"),
                local_path: format!("C:\\work\\{tag}"),
                brief_id: format!("brf_{tag}"),
                brief_body: "Body".to_string(),
                brief_source: "TEST".to_string(),
                event_id: format!("evt_{tag}_genesis"),
                created_at: "2026-10-04T00:00:00Z".to_string(),
            })
            .expect("project");
        storage
    }

    /// A legal MCF-v2 envelope, in the shape `crates/bus/tests/common` assembles, plus the two optional context
    /// fields this operation has to carry forward.
    ///
    /// Assembled by concatenation for the same reason that builder is: a `format!` over a whole envelope makes
    /// the braces unreadable and hides which field is which.
    fn envelope(message_id: &str, project_id: &str, sequence: i64, message_type: &str) -> String {
        [
            r#"{"protocol_version":"MCF-2","schema_version":"2.0.0","#.to_string(),
            format!(r#""message_id":"{message_id}","event_id":"src_{message_id}","#),
            format!(r#""project_id":"{project_id}","session_id":"sess_1","#),
            r#""sender":{"actor_type":"MAYASABA","actor_id":"controller"},"#.to_string(),
            r#""recipients":[{"actor_type":"AGENT","actor_id":"hermes-1"}],"#.to_string(),
            format!(r#""channel":"task","message_type":"{message_type}","phase":"IMPLEMENTATION","#),
            format!(r#""correlation_id":"corr_1","sequence":{sequence},"project_epoch":0,"#),
            r#""priority":"TASK_CONTROL","#.to_string(),
            r#""created_at":"2026-10-04T00:00:05Z","#.to_string(),
            r#""requires_ack":true,"requires_response":false,"blocking":false,"#.to_string(),
            r#""payload":{},"#.to_string(),
            r#""security":{"classification":"INTERNAL_PROJECT","secret_refs":[]},"#.to_string(),
            r#""context_snapshot_id":"ctx_original","state_digest":"a3f1c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956","#.to_string(),
            r#""operation_id":"op_replay"}"#.to_string(),
        ]
        .concat()
    }

    /// Put a message straight into a terminal state, which is what the retry budget's exhaustion produces.
    ///
    /// The bus's own replay tests drive a message there through the declared edge. This reaches the same durable
    /// state directly, because what is under test here is the judgement made *about* a terminal message, not the
    /// expiry path that produced it - and `replay_source` reads the same row either way.
    fn force_expired(storage: &Storage, message_id: &str) {
        storage
            .conn()
            .execute(
                "UPDATE messages SET delivery_state = 'EXPIRED' WHERE message_id = ?1",
                [message_id],
            )
            .expect("expire");
    }

    fn insert_message(storage: &Storage, project_id: &str, message_id: &str, message_type: &str) {
        storage
            .conn()
            .execute(
                "INSERT INTO messages (message_id, event_id, project_id, session_id, message_type, channel,
                                       sequence, correlation_id, causation_id, idempotency_key, delivery_state,
                                       envelope_json, created_at)
                 VALUES (?1, ?2, ?3, 'sess_1', ?4, 'task', 1, 'corr_1', NULL, NULL, 'EXPIRED', ?5,
                         '2026-10-04T00:00:05Z')",
                rusqlite::params![
                    message_id,
                    format!("src_{message_id}"),
                    project_id,
                    message_type,
                    envelope(message_id, project_id, 1, message_type)
                ],
            )
            .expect("insert message");
    }

    fn state_of(storage: &Storage, message_id: &str) -> String {
        storage
            .delivery_state(message_id)
            .expect("read state")
            .expect("row exists")
    }

    fn message_count(storage: &Storage) -> i64 {
        storage
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("count")
    }

    #[test]
    fn a_material_action_is_refused_and_nothing_is_enqueued() {
        // The whole point of the operation. Replaying a material action would authorize one, and the
        // authorization judgement does not exist, so the replay must not happen at all - not happen and then be
        // reported as a refusal.
        let storage = storage_with_project("material");
        insert_message(&storage, "prj_material", "msg_accept", "TASK_ACCEPT");
        let mut bus = Bus::new(storage);

        let refusal =
            RecoveryService::replay_dead_letter(&mut bus, &CoreIdSource, &CoreClock, "msg_accept")
                .expect_err("a material action must be refused");

        match &refusal {
            ReplayRefusal::AuthorizationNotImplemented {
                message_id,
                message_type,
            } => {
                assert_eq!(message_id, "msg_accept");
                // The type is reported, so the caller can see what was refused rather than only that something
                // was - which is what lets the UI name the operation it will not replay.
                assert_eq!(message_type, "TASK_ACCEPT");
            }
            other => panic!("expected an authorization refusal, got {other:?}"),
        }

        // Nothing was produced, and the source is exactly as it was.
        assert_eq!(message_count(bus.storage()), 1);
        assert_eq!(state_of(bus.storage(), "msg_accept"), "EXPIRED");
    }

    #[test]
    fn a_non_material_terminal_message_is_replayed_as_a_new_message() {
        let storage = storage_with_project("allowed");
        let mut bus = Bus::new(storage);
        bus.enqueue(&envelope("msg_task", "prj_allowed", 1, "TASK"))
            .expect("enqueue");
        force_expired(bus.storage(), "msg_task");

        let outcome =
            RecoveryService::replay_dead_letter(&mut bus, &CoreIdSource, &CoreClock, "msg_task")
                .expect("a non-material message is replayable");

        // A new message, not a reset: the original keeps its identity and its terminal state.
        assert_ne!(outcome.replayed_message_id, "msg_task");
        assert_eq!(outcome.source_message_id, "msg_task");
        assert_eq!(state_of(bus.storage(), "msg_task"), "EXPIRED");
        assert_eq!(message_count(bus.storage()), 2);
        assert_eq!(
            state_of(bus.storage(), &outcome.replayed_message_id),
            "QUEUED"
        );
    }

    #[test]
    fn the_response_carries_the_original_context_and_never_claims_a_refresh() {
        // A replay re-enqueues the original envelope, so the context it carries is the original's. Claiming a
        // refresh would let a caller act on a stale context believing it current.
        let storage = storage_with_project("context");
        let mut bus = Bus::new(storage);
        bus.enqueue(&envelope("msg_ctx", "prj_context", 1, "TASK"))
            .expect("enqueue");
        force_expired(bus.storage(), "msg_ctx");

        let outcome =
            RecoveryService::replay_dead_letter(&mut bus, &CoreIdSource, &CoreClock, "msg_ctx")
                .expect("replay");

        assert_eq!(outcome.context_snapshot_id.as_deref(), Some("ctx_original"));
        assert_eq!(
            outcome.state_digest.as_deref(),
            Some("a3f1c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956")
        );
        assert!(!outcome.context_refreshed);
        // The first replay of this operation did create a message, so it is not a deduplication.
        assert!(!outcome.deduplicated);
    }

    #[test]
    fn a_second_replay_produces_no_second_message() {
        // Idempotence is a property of the identity the replay keeps, not of the id it invents: the envelope
        // carries the original's `operation_id`, so the second enqueue matches the first and is absorbed.
        let storage = storage_with_project("idempotent");
        let mut bus = Bus::new(storage);
        bus.enqueue(&envelope("msg_twice", "prj_idempotent", 1, "TASK"))
            .expect("enqueue");
        force_expired(bus.storage(), "msg_twice");

        let first =
            RecoveryService::replay_dead_letter(&mut bus, &CoreIdSource, &CoreClock, "msg_twice")
                .expect("first replay");
        let second =
            RecoveryService::replay_dead_letter(&mut bus, &CoreIdSource, &CoreClock, "msg_twice")
                .expect("second replay");

        assert_eq!(second.replayed_message_id, first.replayed_message_id);
        assert!(
            second.deduplicated,
            "the second replay must report absorption"
        );
        // Source plus one replay. A second message here would be a second delivery of one operation.
        assert_eq!(message_count(bus.storage()), 2);
    }

    #[test]
    fn a_message_that_is_not_there_is_refused_without_enqueueing() {
        let storage = storage_with_project("missing");
        let mut bus = Bus::new(storage);

        let refusal =
            RecoveryService::replay_dead_letter(&mut bus, &CoreIdSource, &CoreClock, "msg_absent")
                .expect_err("an absent message cannot be replayed");

        match &refusal {
            // Reported as unreadable rather than as a storage failure: nothing went wrong with the database.
            ReplayRefusal::Unreadable { message_id, .. } => assert_eq!(message_id, "msg_absent"),
            other => panic!("expected an unreadable refusal, got {other:?}"),
        }
        assert_eq!(message_count(bus.storage()), 0);
    }
}
