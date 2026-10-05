//! Controller-owned bridge from agent runtime facts to the durable MCF-v2 bus.
//!
//! This module coordinates existing authorities only:
//! - Agent adapters build/normalize native runtime data.
//! - Protocol validates the canonical envelope.
//! - Bus owns durable delivery state.
//! - PolicyService remains responsible for material authorization.
//!
//! No state machine or storage authority is introduced here.

use mayasaba_agents::{build_handshake_envelope, HandshakeEnvelopeInput};
use mayasaba_bus::{Bus, BusError};
use mayasaba_storage::EnqueuedMessage;

#[derive(Debug)]
pub enum AgentGatewayError {
    Adapter(mayasaba_agents::AdapterError),
    Bus(BusError),
}

impl From<mayasaba_agents::AdapterError> for AgentGatewayError {
    fn from(value: mayasaba_agents::AdapterError) -> Self {
        Self::Adapter(value)
    }
}

impl From<BusError> for AgentGatewayError {
    fn from(value: BusError) -> Self {
        Self::Bus(value)
    }
}

/// Queue an agent HANDSHAKE only after the adapter has constructed a protocol-valid envelope.
///
/// Delivery remains durable and controller-driven; this function does not dispatch or activate the agent.
pub fn queue_agent_handshake(
    bus: &mut Bus,
    input: &HandshakeEnvelopeInput,
) -> Result<EnqueuedMessage, AgentGatewayError> {
    let envelope = build_handshake_envelope(input)?;
    Ok(bus.enqueue(&envelope)?)
}
