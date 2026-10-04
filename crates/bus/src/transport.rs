//! The transport boundary: the one place the bus hands a message to something outside itself.
//!
//! `docs/MCF-V2-IMPLEMENTATION-DESIGN.md` says "The initial implementation is local-only. The transport
//! abstraction must not imply a remote/cloud executor." This trait is therefore deliberately the narrowest
//! thing that can carry an envelope: it takes the stored text and returns whether the handover happened. It
//! knows nothing about projects, retries, states or the database, so nothing about the durable machine leaks
//! into an adapter and nothing about an adapter leaks into the machine.
//!
//! # Why the failure vocabulary is the adapter's
//!
//! A failed handover is not a storage failure and not a domain rejection, and the registry already had the
//! words for what it actually is. The three variants map to `ADAPTER_UNAVAILABLE` ("the local agent CLI this
//! message is addressed to is not running or cannot be started"), `TIMEOUT` ("an operation did not complete
//! within its declared time budget") and `ADAPTER_PROTOCOL_ERROR` ("a local agent CLI answered with something
//! that is not MCF"). No new registry code was needed, and `TRANSPORT_FAILURE` is deliberately not among them:
//! its registered meaning is the frontend side of the Tauri boundary and its `mcf_code` is `null`, so a bus
//! that produced it would be reporting a frontend condition it cannot observe.

/// What went wrong when the bus tried to hand a message over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The recipient's adapter is not running or cannot be started.
    Unavailable(String),
    /// The adapter did not complete the handover within its time budget.
    Timeout(String),
    /// The adapter answered with something that is not MCF.
    ProtocolError(String),
}

impl TransportError {
    /// The canonical registry key for this failure.
    pub fn code(&self) -> &'static str {
        match self {
            TransportError::Unavailable(_) => "ADAPTER_UNAVAILABLE",
            TransportError::Timeout(_) => "TIMEOUT",
            TransportError::ProtocolError(_) => "ADAPTER_PROTOCOL_ERROR",
        }
    }

    /// The detail to record in the attempt's `error_json`.
    pub fn detail(&self) -> &str {
        match self {
            TransportError::Unavailable(d)
            | TransportError::Timeout(d)
            | TransportError::ProtocolError(d) => d,
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for TransportError {}

/// Somewhere a persisted message can be handed to.
///
/// Implemented by tests and, in production, by whatever adapter boundary exists at the time. The bus holds a
/// `&mut dyn Transport` rather than a concrete type, so the durable machine can be tested without an adapter and
/// an adapter can be tested without the database.
pub trait Transport {
    /// Hand one stored envelope over.
    ///
    /// Takes the text rather than a parsed value because that is what the durable record holds: what is sent
    /// must be what was stored, or the record of what happened describes a different message.
    ///
    /// `Ok(())` means the transport took the message. It does not mean the recipient processed it - ACK means
    /// receipt, not successful execution (AGENTS.md section 7) - and it does not mean the recipient was even
    /// reachable beyond the point the adapter accepted it.
    fn send(&mut self, envelope_json: &str) -> Result<(), TransportError>;
}
