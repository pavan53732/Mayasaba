//! Structured bus errors, and the canonical registry code each one carries.
//!
//! Every variant maps to a code in `schemas/error-v1/registry.json` (DEC-055). The mapping is a total function
//! over the enum rather than a string field, so a variant cannot be constructed carrying a code that does not
//! describe it, and adding a variant without deciding its code does not compile.
//!
//! The contract gate reads this function: `tools/contracts/verify.mjs` names this file as one of the
//! error-code mapping sites, so a code named here that the registry does not register fails verification. That
//! is why the arms are written as bare `=> "CODE"` and why the function keeps its name.

use mayasaba_protocol::envelope::EnvelopeRejection;
use mayasaba_storage::StorageError;

/// Why a bus operation did not complete.
#[derive(Debug)]
pub enum BusError {
    /// The text is not a legal MCF-v2 envelope, so there is nothing to deliver.
    InvalidEnvelope(EnvelopeRejection),
    /// A retry reused an idempotency identity with a different request body.
    ///
    /// The scope is `project_id + operation_id` (DEC-027, and the implementation design's "Idempotency scope"),
    /// and the body is the envelope's `payload`. Both halves matter: the same operation with the same body is a
    /// retry and is absorbed, and the same operation with a different body is two different requests that
    /// cannot both be applied at one position.
    IdempotencyConflict {
        /// The message the enqueue matched, whether by its own id or through the operation scope.
        message_id: String,
        /// The operation the two enqueues shared, when the match was made through the operation scope.
        operation_id: Option<String>,
    },
    /// The envelope names a project that does not exist.
    UnknownProject { project_id: String },
    /// A different message already occupies the ordering position this one claims.
    SequenceConflict {
        session_id: String,
        channel: String,
        sequence: i64,
    },
    /// An arriving message reuses a `message_id` that a message this bus already knows is using.
    ///
    /// Distinct from a redelivery, which is the same message arriving twice and is answered from the inbox. This
    /// is two different messages claiming one identity, and it is refused rather than absorbed: absorbing it
    /// would mean the second message's contents silently replaced the first's record under the first's id.
    DuplicateMessage { message_id: String },
    /// A stored value does not satisfy what the schema declares of its column.
    Malformed { column: String, detail: String },
    /// The durable write failed, so the message is not queued and nothing may be assumed about it.
    Storage(StorageError),
    /// The message terminated because its delivery-attempt budget is exhausted.
    ///
    /// Reported rather than swallowed, because an expiry is a real outcome: the message will never be delivered
    /// and a dead letter now records why. It is not something a caller can retry - the budget is spent.
    Expired {
        message_id: String,
        attempts: i64,
        /// The registry code of the last failure, which is what the dead letter records.
        last_error: &'static str,
    },
    /// The queue is at its configured bound, so the work was not accepted.
    ///
    /// Reported rather than swallowed, and distinct from a policy refusal: `POLICY_DENIED` means the request may
    /// never be made, and this means it may not be made yet. The two are different answers and a caller acts
    /// differently on each, so collapsing them into one code would cost the caller the ability to tell them
    /// apart (AGENTS.md section 19).
    CapacityExceeded {
        /// How many entries were already waiting for dispatch.
        pending: i64,
        /// The configured bound they had reached.
        limit: i64,
    },
}

/// Turn a storage refusal into the bus's own report of it.
///
/// Storage names the condition; the bus names the registry code. A blanket `From<StorageError>` would file every
/// refusal under `STORAGE_FAILURE`, whose meaning is "the durable source of truth could not be read or written" -
/// which is untrue of a duplicate position and of an unknown project. Storage read the database perfectly well
/// in both cases; it refused the write on purpose. Filing those under an internal failure would also tell the
/// caller to back off and retry, which can never succeed for either.
///
/// The match is exhaustive with no catch-all arm naming a code of its own, so adding a `StorageError` variant
/// forces a decision here rather than silently inheriting `STORAGE_FAILURE`.
pub(crate) fn classify(e: StorageError) -> BusError {
    match e {
        StorageError::SequenceConflict {
            session_id,
            channel,
            sequence,
        } => BusError::SequenceConflict {
            session_id,
            channel,
            sequence,
        },
        StorageError::UnknownProject { project_id } => BusError::UnknownProject { project_id },
        StorageError::MalformedJson { column } => BusError::Malformed {
            column,
            detail: "must hold JSON text, and this is not JSON".to_string(),
        },
        StorageError::Malformed { column, detail } => BusError::Malformed { column, detail },
        // Everything else is the database itself failing, which is what STORAGE_FAILURE describes. The
        // variants listed here are named rather than absorbed so that a new one has to be considered.
        StorageError::Schema(_)
        | StorageError::Db(_)
        | StorageError::UnscopedEvent
        | StorageError::Canonical(_)
        | StorageError::StaleFence { .. }
        | StorageError::InvalidAttemptTransition { .. } => BusError::Storage(e),
        // A named row that does not exist is not a storage failure: nothing went wrong with the database, and
        // reporting it as one would send a caller looking for a broken disk. It is the request naming something
        // the durable record does not hold, which is the schema-invalid condition - the same family as a column
        // that does not hold what the contract declares of it.
        StorageError::NotFound(detail) => BusError::Malformed {
            column: "messages.message_id".to_string(),
            detail,
        },
        // Only a terminal message can be replayed, and the declared machine has no edge from an in-flight state
        // back to `CREATED`. Asking to replay one is therefore a request the contract cannot express, which is the
        // schema-invalid condition - not a storage failure, since nothing failed to be stored.
        StorageError::NotTerminal {
            message_id,
            delivery_state,
        } => BusError::Malformed {
            column: "messages.delivery_state".to_string(),
            detail: format!(
                "message {message_id} is {delivery_state}, which is not terminal, so it cannot be replayed"
            ),
        },
    }
}

impl BusError {
    /// The canonical registry key for this error (DEC-055).
    pub fn code(&self) -> &'static str {
        match self {
            BusError::InvalidEnvelope(_) => "SCHEMA_INVALID",
            BusError::IdempotencyConflict { .. } => "IDEMPOTENCY_CONFLICT",
            BusError::UnknownProject { .. } => "PROJECT_MISMATCH",
            // Not SEQUENCE_GAP: nothing is missing. Two distinct messages claim one place, which is the
            // condition MCF_DUPLICATE_CONFLICT names and MCF_SEQUENCE_GAP does not.
            BusError::SequenceConflict { .. } => "DUPLICATE_CONFLICT",
            // Two messages claiming one identity is the same condition as two claiming one position: the
            // registry's `DUPLICATE_CONFLICT` is "a duplicate was detected where the contract forbids one", and
            // the contract forbids a `message_id` naming two messages.
            BusError::DuplicateMessage { .. } => "DUPLICATE_CONFLICT",
            BusError::Malformed { .. } => "SCHEMA_INVALID",
            BusError::Storage(_) => "STORAGE_FAILURE",
            // The message expired, so the condition is that it is dead-lettered and will not be retried. The
            // registry's own meaning for this code is exactly that: the message exhausted its retry budget and
            // is retained for inspection.
            BusError::Expired { .. } => "DEAD_LETTERED",
            // Transient and expected, which is why it is not POLICY_DENIED: see the variant.
            BusError::CapacityExceeded { .. } => "CAPACITY_EXCEEDED",
        }
    }
}

impl std::fmt::Display for BusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusError::InvalidEnvelope(e) => write!(f, "{e}"),
            BusError::IdempotencyConflict {
                message_id,
                operation_id,
            } => match operation_id {
                Some(operation) => write!(
                    f,
                    "operation `{operation}` was already enqueued as message `{message_id}` with a different \
                     payload, so the two cannot both be applied"
                ),
                None => write!(
                    f,
                    "message `{message_id}` was already enqueued with a different payload, so the two cannot \
                     both be applied"
                ),
            },
            BusError::UnknownProject { project_id } => {
                write!(f, "no project `{project_id}` exists to route to")
            }
            BusError::SequenceConflict {
                session_id,
                channel,
                sequence,
            } => write!(
                f,
                "sequence {sequence} on channel `{channel}` for session `{session_id}` is already occupied by \
                 a different message"
            ),
            BusError::DuplicateMessage { message_id } => write!(
                f,
                "message `{message_id}` already names a different message, so this one cannot reuse the \
                 identity: a redelivery of the same message is answered from the inbox instead"
            ),
            BusError::Malformed { column, detail } => {
                write!(f, "stored value in `{column}` is not usable: {detail}")
            }
            BusError::Storage(e) => write!(f, "{e}"),
            BusError::Expired {
                message_id,
                attempts,
                last_error,
            } => write!(
                f,
                "message `{message_id}` exhausted its delivery budget after {attempts} attempt(s); the last \
                 failure was {last_error} and a dead letter records it"
            ),
            BusError::CapacityExceeded { pending, limit } => write!(
                f,
                "{pending} queue entries are already waiting for dispatch, which is the configured bound of \
                 {limit}; the work was not accepted and may be retried once the queue drains"
            ),
        }
    }
}

impl std::error::Error for BusError {}
