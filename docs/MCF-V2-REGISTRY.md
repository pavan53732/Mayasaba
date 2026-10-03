# MCF-v2 Registry

## Authority

This document is the human-readable index of the machine-readable MCF-v2 registry at `schemas/mcf-v2/registry.json`. The JSON registry is the compile/test source of truth; this document explains its ownership and mapping.

## Priority lanes

1. `EMERGENCY_CONTROL`
2. `SYNCHRONIZATION`
3. `TASK_CONTROL`
4. `FAILURE_RECOVERY`
5. `COUNCIL`
6. `PROGRESS_HEARTBEAT`
7. `BULK`

## Message registry

### Lifecycle/control
`HANDSHAKE`, `HANDSHAKE_ACK`, `READY`, `HEARTBEAT`, `ACK`, `NACK`, `PAUSE`, `RESUME`, `CANCEL`, `STOP`, `ERROR`, `RETRY`, `DEAD_LETTER`

### Synchronization
`SYNC_REQUEST`, `SYNC_RESPONSE`, `STATE_DIGEST`, `CONTEXT_UPDATE`, `STALE_CONTEXT`, `EPOCH_CHANGED`

### Council
`IDEA`, `PROPOSAL`, `QUESTION`, `CRITIQUE`, `COUNTERARGUMENT`, `REBUTTAL`, `REVISION`, `AGREE`, `DISAGREE`, `BLOCK`, `ACCEPT`, `REJECT`, `ABSTAIN`, `DECISION`, `LOCK`, `SYNTHESIS`

### Task/handoff
`TASK`, `TASK_ACCEPT`, `TASK_REJECT`, `TASK_LEASE`, `TASK_LEASE_RENEW`, `TASK_RELEASE`, `TASK_PROGRESS`, `HANDOFF_REQUEST`, `HANDOFF_ACCEPT`, `HANDOFF_REJECT`

### Engineering/validation
`IMPLEMENTATION_REPORT`, `FAILURE`, `DIAGNOSIS`, `REPAIR_REQUEST`, `REPAIR_RESULT`, `REVIEW`, `TEST_RESULT`, `VALIDATION`, `CERTIFICATION`

### Execution/evidence
`EXECUTION_REQUEST`, `EXECUTION_STARTED`, `EXECUTION_RESULT`, `ARTIFACT_PUBLISHED`, `EVIDENCE_PUBLISHED`

## Material-action event registry

The material-action chain is no longer prose-only. Each stage maps to a canonical event type:

`INTENT_RECORDED → ROUTE_RESOLVED → MESSAGE_PERSISTED → MESSAGE_QUEUED → MESSAGE_DISPATCHED → MESSAGE_RECEIVED → MESSAGE_ACKED → CONTEXT_VALIDATED → AUTHORIZATION_VALIDATED → LEASE_VALIDATED → ACTION_STARTED → ACTION_PROGRESS → ACTION_COMPLETED/ACTION_FAILED → RESULT_PERSISTED → EVIDENCE_CAPTURED → VALIDATION_REQUESTED → VALIDATION_COMPLETED → STATE_COMMITTED/REPAIR_STARTED → PARTICIPANTS_SYNCED → TASK_CYCLE_CLOSED`.

The complete event enum is in `schemas/mcf-v2/registry.json`.

Workspace and integration admission emit `ADMISSION_RECORDED` (owner `crates/workspace`), which carries the admission verdict for either gate. It is not a stage in the material-action chain; it is the durable record that a gate decided.

## Registry ownership

- `crates/protocol`: schema/types/registry validation.
- `crates/bus`: delivery events and delivery-state transitions.
- `crates/core`: material-action transitions and controller lifecycle events.
- Domain crates own domain-specific event payload construction but cannot invent a second registry.

## Transition registry

Every legal state transition is represented by `schemas/mcf-v2/transition-types.json`. Its owner is the authoritative subsystem for the state machine; `crates/core` enforces cross-machine orchestration and transaction ordering.

No implementation may introduce a stringly-typed message/event/transition name outside this registry.

A transition record's `event_type` must be a canonical MCF event from `event-types.schema.json`, and every record sharing a `transition_id` must name the same one — `registry.json`'s `transition_event_rules` requires exactly one canonical event type per transition. `emitted_events` is exactly `[event_type]`. The local contract gate (`npm run verify:contracts`) enforces all three; before 2026-10-03 it read no transition record's `event_type` at all, and 48 records violated the rule undetected — 32 named a Tauri UI event rather than an MCF event, and 16 named a real MCF event belonging to a different edge of the same machine.
