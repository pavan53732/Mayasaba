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

pub mod clock;
pub mod dispatch;
pub mod error;
mod id;
pub mod policy;
pub mod transport;

pub use clock::{Clock, FixedClock};
pub use dispatch::{decide, DispatchDecision};
pub use error::BusError;
pub use id::{FixedIdSource, IdSource};
pub use policy::{BackoffPolicy, DispatchPolicy};
pub use transport::{Transport, TransportError};

use mayasaba_protocol::envelope::{parse_envelope, Envelope};
use mayasaba_storage::{
    DeliveryAttempt, EnqueuedMessage, IncomingMessage, NewOutboundMessage, Storage,
};

/// Mayasaba durable bus crate boundary.
pub const CRATE_NAME: &str = "mayasaba-bus";

/// What one dispatch pass did.
///
/// Returned rather than logged, because the caller decides whether a pass is observable and this crate performs
/// no I/O. The counts are derived from the outcomes, so they cannot disagree with them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DispatchReport {
    /// One entry per queue entry the pass claimed, in the order it was claimed.
    pub outcomes: Vec<DispatchOutcome>,
    /// How much dispatch work was still outstanding when the pass ended.
    ///
    /// `batch_size` bounds how much **one pass** does; this is what bounds the **queue**, by making the backlog
    /// visible to whoever produces into it. The bus measures and the caller decides, which is the same shape as
    /// requeue, expiry and gap detection - and it is the only shape available here, because the error registry has
    /// no code for transient capacity (see DEC-064): the bus cannot refuse an enqueue for being too busy without
    /// either inventing a code or reusing `POLICY_DENIED`, and reusing it would make a permanent denial
    /// indistinguishable from a temporary one.
    pub remaining: i64,
}

/// What happened to one claimed queue entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Handed to the transport, and the message advanced `QUEUED -> DISPATCHED`.
    Dispatched { message_id: String, attempt_no: i64 },
    /// The transport refused it. The message is still `QUEUED` and the next attempt is scheduled.
    Failed {
        message_id: String,
        attempt_no: i64,
        /// The registry code of the transport's refusal.
        code: &'static str,
        /// Seconds until the next attempt.
        retry_in_seconds: i64,
    },
    /// The attempt budget was exhausted, so the message terminated `QUEUED -> EXPIRED` with a dead letter.
    Expired { message_id: String, attempts: i64 },
}

impl DispatchReport {
    /// How many entries were handed over.
    pub fn dispatched(&self) -> usize {
        self.count(|o| matches!(o, DispatchOutcome::Dispatched { .. }))
    }

    /// How many entries the transport refused.
    pub fn failed(&self) -> usize {
        self.count(|o| matches!(o, DispatchOutcome::Failed { .. }))
    }

    /// How many entries exhausted their budget.
    pub fn expired(&self) -> usize {
        self.count(|o| matches!(o, DispatchOutcome::Expired { .. }))
    }

    fn count(&self, predicate: impl Fn(&DispatchOutcome) -> bool) -> usize {
        self.outcomes.iter().filter(|o| predicate(o)).count()
    }
}

/// The durable communication bus.
pub struct Bus {
    storage: Storage,
    dispatch_policy: DispatchPolicy,
    backoff_policy: BackoffPolicy,
}

impl Bus {
    /// Take ownership of a storage handle.
    ///
    /// Owned rather than borrowed because the bus's operations are transactions: `enqueue` needs `&mut Storage`
    /// for the whole of its work, and a caller holding a second handle to the same connection could interleave
    /// with it.
    pub fn new(storage: Storage) -> Self {
        Bus {
            storage,
            dispatch_policy: DispatchPolicy::default(),
            backoff_policy: BackoffPolicy::default(),
        }
    }

    /// Take ownership of a storage handle and use an explicit policy.
    ///
    /// The policy is an argument rather than something the bus reads from
    /// `schemas/mcf-v2/bus-policies.json`, because reading a file is I/O and this crate performs none. The
    /// defaults are the shipped values; a caller that wants the configuration layer's overrides passes them.
    pub fn with_policy(
        storage: Storage,
        dispatch_policy: DispatchPolicy,
        backoff_policy: BackoffPolicy,
    ) -> Self {
        Bus {
            storage,
            dispatch_policy,
            backoff_policy,
        }
    }

    /// The policy this bus dispatches under.
    pub fn dispatch_policy(&self) -> DispatchPolicy {
        self.dispatch_policy
    }

    /// The policy this bus backs off under.
    pub fn backoff_policy(&self) -> BackoffPolicy {
        self.backoff_policy
    }

    /// The storage handle, for reading back what the bus wrote.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Run one dispatch pass: claim the due queue entries, decide each one's fate from its stored attempt count,
    /// and act.
    ///
    /// `clock` supplies the instant that decides which entries are due, and `transport` is the only thing the
    /// message is handed to. Neither is stored on the bus, because a pass is the unit of work and a caller may
    /// legitimately run two passes with different clocks (a test) or different transports (a failover).
    ///
    /// # The two things this must not do
    ///
    /// **It must not advance `delivery_state` for a failed handover.** The machine declares no such transition,
    /// so a refused message stays `QUEUED` and its failure lives in `message_attempts`. `DISPATCHED` therefore
    /// means "handed to the transport" and never "the transport was tried".
    ///
    /// **It must not retry forever.** The decision is taken from `outbox.attempts`, which is durable, so a
    /// process that crashes on every attempt still runs out of budget. When it does, the message terminates
    /// through the declared `QUEUED -> EXPIRED` edge and a dead letter records the last failure.
    ///
    /// A failure to *persist* what happened is a `BusError` and stops the pass, because the alternative is a
    /// dispatcher that keeps sending messages whose record it cannot write. A failure to *deliver* is not an
    /// error of the pass: it is an outcome, recorded and reported.
    pub fn dispatch_due(
        &mut self,
        clock: &dyn Clock,
        transport: &mut dyn Transport,
    ) -> Result<DispatchReport, BusError> {
        let now = clock.now_rfc3339();
        let due = self
            .storage
            .due_outbound(&now, self.dispatch_policy.batch_size)
            .map_err(error::classify)?;
        let mut report = DispatchReport::default();

        for entry in due {
            match decide(entry.attempts, &self.dispatch_policy) {
                DispatchDecision::Expire { attempts } => {
                    // An expiry is not a delivery attempt, so it writes no `message_attempts` row: that table
                    // records what was tried, and nothing is tried here - the decision is that trying is over.
                    // The dead letter therefore reports the last *real* failure, and the count comes from
                    // `outbox.attempts`, which is the durable record of how many attempts were made.
                    self.storage
                        .expire_message(&entry, &now)
                        .map_err(error::classify)?;
                    report.outcomes.push(DispatchOutcome::Expired {
                        message_id: entry.message_id,
                        attempts,
                    });
                }
                DispatchDecision::Send { attempt_no } => {
                    let started_at = clock.now_rfc3339();
                    match transport.send(&entry.envelope_json) {
                        Ok(()) => {
                            let attempt = DeliveryAttempt {
                                attempt_no,
                                started_at,
                                finished_at: clock.now_rfc3339(),
                                outcome: "SENT".to_string(),
                                error_code: None,
                                error_detail: None,
                            };
                            self.storage
                                .record_dispatched(&entry, &attempt)
                                .map_err(error::classify)?;
                            report.outcomes.push(DispatchOutcome::Dispatched {
                                message_id: entry.message_id,
                                attempt_no,
                            });
                        }
                        Err(failure) => {
                            let retry_in_seconds = self.backoff_policy.delay_seconds(attempt_no);
                            let attempt = DeliveryAttempt {
                                attempt_no,
                                started_at,
                                finished_at: clock.now_rfc3339(),
                                outcome: "TRANSPORT_REFUSED".to_string(),
                                error_code: Some(failure.code().to_string()),
                                error_detail: Some(failure.detail().to_string()),
                            };
                            self.storage
                                .record_failed_attempt(
                                    &entry.message_id,
                                    &attempt,
                                    retry_in_seconds,
                                )
                                .map_err(error::classify)?;
                            report.outcomes.push(DispatchOutcome::Failed {
                                message_id: entry.message_id,
                                attempt_no,
                                code: failure.code(),
                                retry_in_seconds,
                            });
                        }
                    }
                }
            }
        }

        // Read after the pass, so the number describes the queue as the pass left it rather than as it found it.
        report.remaining = self
            .storage
            .pending_outbound_count()
            .map_err(error::classify)?;
        Ok(report)
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

        // Refused before the write when the queue is already at its bound. A message whose id already exists
        // is never refused for capacity: it adds no work, because the enqueue below absorbs it as a retry. See
        // DEC-068 for the wart this leaves - a message that is new by id but duplicates a live
        // (project_id, operation_id) is refused rather than deduplicated when the queue is full.
        let known = self
            .storage
            .message_exists(&new.message_id)
            .map_err(error::classify)?;
        if !known {
            let pending = self
                .storage
                .pending_outbound_count()
                .map_err(error::classify)?;
            if pending >= self.dispatch_policy.max_pending {
                return Err(BusError::CapacityExceeded {
                    pending,
                    limit: self.dispatch_policy.max_pending,
                });
            }
        }

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

/// What a receive did.
///
/// `duplicate` is the whole point of the inbound side: a redelivery is answered from the durable inbox rather
/// than processed again, so the caller is told which of the two happened and what the message's state already
/// was. A duplicate reports the state the message reached, not the state a fresh receive would have produced,
/// which is what "return the prior terminal outcome" requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub message_id: String,
    pub duplicate: bool,
    pub processing_state: String,
    pub terminal_event_id: Option<String>,
    /// A missing ordering position this arrival revealed, if any.
    ///
    /// A gap is **reported, not refused**: the message is still accepted, because refusing it would discard the
    /// very arrival that makes the gap visible and would leave the receiver unable to ask for what is missing.
    /// The design's answer to a gap is to request a resync, which is a decision the caller takes.
    ///
    /// Nothing stores the gap. The positions are already durable in `messages`, so "a position is missing" is
    /// derived from the record rather than recorded a second time - and a second record of one fact is a second
    /// fact that can disagree.
    pub gap: Option<SequenceGap>,
}

/// An ordering position that no message holds, revealed by one that arrived past it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceGap {
    pub session_id: String,
    pub channel: String,
    /// The position the stream was expected to continue at.
    pub expected: i64,
    /// The position that actually arrived.
    pub found: i64,
}

impl Bus {
    /// Take delivery of an envelope: validate it, persist its identity, then acknowledge receipt of it.
    ///
    /// The order is the design's and it is load-bearing. The envelope is validated by the protocol crate's own
    /// validator rather than by a second opinion written here, the project must exist, and the message's
    /// identity must be new. Only then is anything written, and what is written is the inbox row first and the
    /// acknowledgement second - so a crash between them leaves a message that has been seen and not yet
    /// acknowledged, which a redelivery resolves, rather than an acknowledgement of a message nothing recorded.
    ///
    /// **A redelivery is not an error.** It returns `duplicate: true` and the state the message already reached,
    /// and it writes nothing at all: no second inbox row, no second receipt, no second event. That is what makes
    /// duplicate delivery safe, and it is why the caller must act on `duplicate` rather than on `Ok`.
    ///
    /// Two collisions are refused rather than absorbed, because both mean two different messages claim one
    /// identity. A `message_id` already used by a message this bus dispatched is `DuplicateMessage`; a
    /// `(session_id, channel, sequence)` position already taken is `SequenceConflict`. Both directions share one
    /// ordering space per channel - the channel is a single ordered stream - so a position is not reusable.
    pub fn receive(
        &mut self,
        envelope_json: &str,
        clock: &dyn Clock,
    ) -> Result<Received, BusError> {
        let envelope = parse_envelope(envelope_json).map_err(BusError::InvalidEnvelope)?;
        let message_id = envelope.message_id();
        let project_id = envelope.project_id();
        if !self
            .storage
            .project_exists(project_id)
            .map_err(error::classify)?
        {
            return Err(BusError::UnknownProject {
                project_id: project_id.to_string(),
            });
        }
        let mut redelivery = false;
        if let Some(row) = self
            .storage
            .inbox_row(message_id)
            .map_err(error::classify)?
        {
            // A message that has finished processing is a duplicate: the prior outcome is the answer and nothing
            // is repeated. A message still in flight is not a duplicate - but neither does a redelivery of it add
            // anything, because it was already acknowledged and its record already exists. The delivery state
            // decides, because that is what the machine declares.
            //
            // `DISPATCHED` is the one in-flight state that means "sent again after a requeue": the requeue took it
            // back to `QUEUED`, the dispatcher advanced it to `DISPATCHED`, and the receiver's transitions run a
            // second time. `INSERT OR IGNORE` on the message and inbox rows and an upsert on the receipt are what
            // make that second pass idempotent rather than a second record.
            let terminal = matches!(row.processing_state.as_str(), "PROCESSED" | "DEAD_LETTER");
            let state = self
                .storage
                .delivery_state(message_id)
                .map_err(error::classify)?;
            if terminal || state.as_deref() != Some("DISPATCHED") {
                return Ok(Received {
                    message_id: message_id.to_string(),
                    duplicate: true,
                    processing_state: row.processing_state,
                    terminal_event_id: row.terminal_event_id,
                    gap: None,
                });
            }
            redelivery = true;
        }
        // Both checks below ask whether an identity is already taken by something else, which is the wrong
        // question for a message being delivered again: it is taken by this very message.
        if !redelivery
            && self
                .storage
                .message_exists(message_id)
                .map_err(error::classify)?
        {
            return Err(BusError::DuplicateMessage {
                message_id: message_id.to_string(),
            });
        }
        let session_id = envelope.session_id();
        let channel = envelope.channel();
        let sequence = envelope.sequence();
        if !redelivery
            && self
                .storage
                .message_at_position(session_id, channel, sequence)
                .map_err(error::classify)?
                .is_some()
        {
            return Err(BusError::SequenceConflict {
                session_id: session_id.to_string(),
                channel: channel.to_string(),
                sequence,
            });
        }

        // Gap detection: the stream's highest position is durable, so what is missing is derived rather than
        // stored. A first message in a channel has nothing to be missing from, so it reports no gap.
        let highest = self
            .storage
            .highest_sequence(session_id, channel)
            .map_err(error::classify)?;
        let gap = match highest {
            Some(h) if sequence > h + 1 => Some(SequenceGap {
                session_id: session_id.to_string(),
                channel: channel.to_string(),
                expected: h + 1,
                found: sequence,
            }),
            _ => None,
        };

        let incoming = IncomingMessage {
            message_id: message_id.to_string(),
            event_id: envelope.event_id().to_string(),
            project_id: project_id.to_string(),
            session_id: session_id.to_string(),
            message_type: envelope.message_type().to_string(),
            channel: channel.to_string(),
            sequence,
            correlation_id: envelope.correlation_id().to_string(),
            causation_id: envelope.causation_id().map(str::to_string),
            idempotency_key: envelope.idempotency_key().map(str::to_string),
            // The canonical re-encoding, not the caller's bytes, for the same reason the outbound side stores
            // it: two spellings of one envelope must not be two different durable records.
            envelope_json: envelope.to_json_text(),
            created_at: envelope.created_at().to_string(),
        };
        self.storage
            .receive_message(&incoming, &clock.now_rfc3339())
            .map_err(error::classify)?;
        Ok(Received {
            message_id: message_id.to_string(),
            duplicate: false,
            processing_state: "ACKED".to_string(),
            terminal_event_id: None,
            gap,
        })
    }

    /// Begin processing a message that has been acknowledged: the declared `ACKED -> PROCESSING` edge.
    ///
    /// The side effect belongs between this and one of the two outcomes below, which is why the bus does not
    /// perform it: this crate performs no I/O, and the design puts the side effect outside the durable boundary
    /// so that a crash during it leaves a message that is `PROCESSING` and can be accounted for.
    pub fn start_processing(
        &mut self,
        message_id: &str,
        clock: &dyn Clock,
    ) -> Result<(), BusError> {
        self.storage
            .start_processing(message_id, &clock.now_rfc3339())
            .map_err(error::classify)
    }

    /// Finish processing a message successfully: the declared `PROCESSING -> PROCESSED` edge.
    pub fn complete_processing(
        &mut self,
        message_id: &str,
        clock: &dyn Clock,
    ) -> Result<(), BusError> {
        self.storage
            .complete_processing(message_id, &clock.now_rfc3339())
            .map_err(error::classify)
    }

    /// Put a retryably refused message back in the queue: the declared RETRYING -> QUEUED edge.
    ///
    /// This is the exit RETRYING was missing. Without it a retryably refused message waited forever - worse
    /// than a delay, because nothing recorded that the message was stuck.
    ///
    /// Requeueing is a decision, not a timer: the message becomes immediately due, because the schema stores no
    /// processing-attempt count and a delay derived from nothing would be an invented policy.
    pub fn requeue(&mut self, message_id: &str, clock: &dyn Clock) -> Result<(), BusError> {
        self.storage
            .requeue_message(message_id, &clock.now_rfc3339())
            .map_err(error::classify)
    }

    /// End a message that was refused retryably and never requeued: the declared `RETRYING -> EXPIRED` edge.
    ///
    /// This is the last declared exit the bus did not implement, so every state the machine declares now has a
    /// way out. Like `requeue`, it is a decision rather than a timer: there is no stored processing budget to
    /// expire against, so the caller decides, and a caller who never decides leaves a message visibly `RETRYING`
    /// rather than invisibly stuck.
    pub fn expire_retrying(&mut self, message_id: &str, clock: &dyn Clock) -> Result<(), BusError> {
        self.storage
            .expire_retrying(message_id, &clock.now_rfc3339())
            .map_err(error::classify)
    }

    /// Replay a terminal message as a new one: the controller's explicit decision to try again.
    ///
    /// This is **explicit and controller-invoked**, not automatic. Nothing in this crate replays on its own: a
    /// dead letter is a record that a message failed, and deciding that it is worth another attempt is a judgement
    /// the bus has no standing to make - the same reason requeue and expiry are decisions rather than timers.
    ///
    /// The replay is a **new message**, not a reset. `messages.message_id` is a primary key and the original row
    /// still holds the terminal state that justified the replay, so reusing the identity would rewrite history
    /// rather than add to it; and the declared machine has no `EXPIRED -> *` or `DEAD_LETTER -> *` edge, so there
    /// is no transition to express a reset with even if one were wanted. The new message begins at `CREATED` like
    /// any other and carries the original's terminal event as its `causation_id`, so the log says why it exists.
    ///
    /// The identity comes from the injected `IdSource`, because a production source would have to read a clock or
    /// a random device and this crate performs no I/O.
    pub fn replay(
        &mut self,
        message_id: &str,
        ids: &dyn IdSource,
        clock: &dyn Clock,
    ) -> Result<EnqueuedMessage, BusError> {
        let new_message_id = ids.next_message_id();
        let source = self
            .storage
            .replay_source(message_id, &new_message_id, &clock.now_rfc3339())
            .map_err(error::classify)?;
        self.enqueue(&source.envelope_json)
    }

    /// How much dispatch work is outstanding right now, for a producer that wants to throttle before enqueueing.
    ///
    /// The same fact a dispatch pass reports as `remaining`, readable without running a pass - because a producer
    /// that has to run a pass to discover the backlog has already done the work it wanted to avoid.
    pub fn backlog(&self) -> Result<i64, BusError> {
        self.storage
            .pending_outbound_count()
            .map_err(error::classify)
    }

    /// Refuse a message that was being processed.
    ///
    /// `retryable` chooses which declared path the refusal takes, and it is the only thing that does: a
    /// retryable refusal advances `PROCESSING -> RETRYING` and waits to be requeued, and a terminal one
    /// advances `PROCESSING -> REJECTED -> DEAD_LETTER` and writes a dead letter. **Retryability is therefore
    /// carried by the transition, not by a flag stored beside it**, so a message cannot be marked retryable and
    /// simultaneously be terminal.
    ///
    /// The `reason` is recorded in the event payload, and for the terminal path also as the dead letter's
    /// `final_error_json` under the registry's `PROCESS_FAILED`. `message_receipts` has no column for a reason
    /// and the table is fixed, so the event log - the durable record of why a decision was taken - is where it
    /// belongs rather than in a column invented for it.
    pub fn reject_processing(
        &mut self,
        message_id: &str,
        reason: &str,
        retryable: bool,
        clock: &dyn Clock,
    ) -> Result<(), BusError> {
        self.storage
            .reject_processing(message_id, reason, retryable, &clock.now_rfc3339())
            .map_err(error::classify)
    }
}
