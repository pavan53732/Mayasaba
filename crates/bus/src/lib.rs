//! Mayasaba's durable communication bus: the boundary every MCF-v2 message crosses.
//!
//! The bus owns the `message_delivery` state machine (`schemas/mcf-v2/transition-types.json` declares
//! `owner: crates/bus` for it) and the six tables `docs/SQLITE-DATA-ARCHITECTURE.md` assigns it: `messages`,
//! `message_attempts`, `message_receipts`, `inbox`, `outbox` and `dead_letters`.
//!
//! # What this slice does
//!
//! `enqueue` is the outbound half: validate the envelope, then persist the message, queue it, and record both
//! delivery transitions in one transaction. That is the machine's first two spine steps
//! (`CREATED -> PERSISTED -> QUEUED`), and it is the whole of the durable outbox write.
//!
//! # What the bus is not
//!
//! It is a transport, not an authority. The implementation design's routing and authorization boundary says the
//! bus does transport, schema, project and recipient routing and holds no domain policy dependency - which is
//! why this crate depends on `protocol` and `storage` and on nothing else, and why every transition record for
//! this machine declares `epoch_effect: NONE`. A message that is well-formed, addressed to a real project and
//! not a duplicate is enqueued; whether the action it requests is *permitted* is PolicyService's decision, made
//! when the message is processed, not when it is queued.
//!
//! ACK means receipt, not successful execution (AGENTS.md section 7). Nothing here reports a message as done.
//!
//! # What is not implemented yet, stated rather than implied
//!
//! Dispatch, retry and backoff, dead letters, the durable inbox, receipt handling, sequence-gap detection,
//! priority-lane ordering and backpressure are later slices of this crate. The states they produce
//! (`DISPATCHED`, `RETRYING`, `EXPIRED`, `DEAD_LETTER`, and the whole inbox side) are unreachable today, and
//! `outbox.dispatch_state` therefore only ever holds `PENDING`.

pub mod error;

pub use error::BusError;

use mayasaba_protocol::envelope::{parse_envelope, Envelope};
use mayasaba_storage::{EnqueuedMessage, NewOutboundMessage, Storage};

/// Mayasaba durable bus crate boundary.
pub const CRATE_NAME: &str = "mayasaba-bus";

/// The durable communication bus.
pub struct Bus {
    storage: Storage,
}

impl Bus {
    /// Take ownership of a storage handle.
    ///
    /// Owned rather than borrowed because the bus's operations are transactions: `enqueue` needs `&mut Storage`
    /// for the whole of its work, and a caller holding a second handle to the same connection could interleave
    /// with it.
    pub fn new(storage: Storage) -> Self {
        Bus { storage }
    }

    /// The storage handle, for reading back what the bus wrote.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Validate, persist and queue one outbound message.
    ///
    /// The envelope arrives as text rather than as an already-parsed value, because that is what a transport
    /// receives and because [`Envelope::value`] is public: a bus that accepted an `Envelope` would be trusting a
    /// shape it never checked. Text in, validated envelope out.
    ///
    /// A retry is answered with the original rather than refused, which is the `message_delivery` ledger's own
    /// rule for every one of its transitions: "duplicate transition request returns the existing
    /// transition/outcome without repeating side effects". A retry that carries a *different* payload for the
    /// same operation is refused, because absorbing it would silently discard the second request.
    pub fn enqueue(&mut self, envelope_json: &str) -> Result<EnqueuedMessage, BusError> {
        let envelope = parse_envelope(envelope_json).map_err(BusError::InvalidEnvelope)?;

        let new = NewOutboundMessage {
            message_id: envelope.message_id().to_string(),
            event_id: envelope.event_id().to_string(),
            project_id: envelope.project_id().to_string(),
            session_id: envelope.session_id().to_string(),
            message_type: envelope.message_type().to_string(),
            channel: envelope.channel().to_string(),
            sequence: envelope.sequence(),
            correlation_id: envelope.correlation_id().to_string(),
            causation_id: envelope.causation_id().map(str::to_string),
            idempotency_key: envelope.idempotency_key().map(str::to_string),
            operation_id: envelope.operation_id().map(str::to_string),
            // The canonical re-encoding, not the caller's bytes. Storing the caller's text verbatim would make
            // two spellings of one message compare unequal, and the idempotency check below compares text.
            envelope_json: envelope.to_json_text(),
            created_at: envelope.created_at().to_string(),
            project_epoch: envelope.project_epoch(),
        };

        let enqueued = self
            .storage
            .enqueue_message(&new)
            .map_err(error::classify)?;

        if enqueued.deduplicated {
            self.confirm_retry(&envelope, &new, &enqueued)?;
        }

        Ok(enqueued)
    }

    /// Decide whether a deduplicated enqueue was a retry or a conflict.
    ///
    /// Split out because it is the one place the bus compares two documents, and the comparison is the whole
    /// point of the check: `project_id + operation_id` identifies the operation, and the payload identifies the
    /// request. Same operation and same payload is the retry the ledger says to absorb; same operation and a
    /// different payload is two requests at one position, which `IDEMPOTENCY_CONFLICT` says cannot both apply.
    fn confirm_retry(
        &self,
        envelope: &Envelope,
        new: &NewOutboundMessage,
        enqueued: &EnqueuedMessage,
    ) -> Result<(), BusError> {
        let Some(stored) = enqueued.stored_payload_json.as_deref() else {
            // No stored payload to compare against. That means the matched row's envelope carried no `payload`
            // member, which the contract requires of every envelope - so the row was written outside the bus.
            // Refusing is the conservative reading: absorbing the enqueue would apply a request whose original
            // cannot be inspected.
            return Err(BusError::IdempotencyConflict {
                message_id: enqueued.message_id.clone(),
                operation_id: new.operation_id.clone(),
            });
        };
        if stored == envelope.payload_json_text() {
            return Ok(());
        }
        Err(BusError::IdempotencyConflict {
            message_id: enqueued.message_id.clone(),
            operation_id: new.operation_id.clone(),
        })
    }
}
