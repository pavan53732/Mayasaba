//! Mayasaba MCF-v2 protocol crate boundary.
//!
//! This crate is the typed Rust surface of the MCF-v2 contract. The machine, state, event and command
//! surface is generated from the machine-readable registries rather than hand-written, so the crate cannot
//! drift from the contract it is supposed to encode.
//!
//! Generation inputs are `schemas/mcf-v2/transition-types.json` (`machines[]`) and
//! `schemas/mcf-v2/registry.json`, deliberately not `transitions[]`. See `tools/codegen/generate-protocol.mjs`
//! for why the transitions ledger is the wrong input.
//!
//! Per-edge transition data is not generated. `branches` declares an edge as legal but carries no command,
//! event or owner, so the generated surface stops at machine shape. Anything needing per-edge data must read
//! `transitions[]` at runtime, which is a real limitation of the current declaration model and the reason
//! `state_events` remains worth doing (DEC-042 limitation 3).

pub mod generated {
    //! Generated from the machine-readable contract. Do not edit by hand.
    pub mod machines;
}

pub use generated::machines::{
    owner_crate, spine, state_count, states, unreviewed_branches, branches, Machine, AGENT_TYPES,
    EVENT_TYPES, SERVICE_EMITTED_EVENTS, TRANSITION_COMMANDS, TRANSITION_EMITTED_EVENTS,
};