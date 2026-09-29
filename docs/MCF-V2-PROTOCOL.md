# MCF-v2 Protocol Contract

MCF-v2 is the single canonical communication protocol between Mayasaba and the four supported CLI agents.

## 1. Communication path

No uncontrolled direct CLI-to-CLI channel exists.

~~~text
Agent
  ↓
Adapter
  ↓
MCF-v2 Envelope
  ↓
Durable Bus
  ↓
Route / Policy / Context / Lease Gates
  ↓
Recipient Adapter
  ↓
Agent
~~~

## 2. End-to-end material action

~~~text
INTENT_RECORDED
→ ROUTE_RESOLVED
→ MESSAGE_PERSISTED
→ MESSAGE_QUEUED
→ MESSAGE_DISPATCHED
→ MESSAGE_RECEIVED
→ ACKED
→ CONTEXT_VALIDATED
→ AUTHORIZATION_VALIDATED
→ LEASE_VALIDATED
→ ACTION_STARTED
→ PROGRESS
→ ACTION_COMPLETED / ACTION_FAILED
→ RESULT_PERSISTED
→ EVIDENCE_CAPTURED
→ VALIDATION_REQUESTED
→ VALIDATION_COMPLETED
→ STATE_COMMITTED / REPAIR_STARTED
→ AFFECTED_PARTICIPANTS_SYNCED
→ TASK/CYCLE_CLOSED
~~~

ACK is only transport/session receipt.

## 3. Envelope

Required fields include:

protocol_version, schema_version, message_id, event_id, project_id, session_id, sender, recipients, channel, message_type, phase, correlation_id, sequence, project_epoch, priority, created_at, requires_ack, requires_response, blocking, payload and security.

Conditional fields include:

causation_id, task_id, round_id, context_snapshot_id, state_digest, idempotency_key and expires_at.

## 4. Delivery lifecycle

~~~text
CREATED → PERSISTED → QUEUED → DISPATCHED → RECEIVED → ACKED → PROCESSING → PROCESSED
~~~

Failure branches:

- RETRYING
- EXPIRED
- REJECTED
- DEAD_LETTER

Use at-least-once delivery and receiver-side idempotency. Exactly-once execution is not assumed.

## 5. Causality

correlation_id identifies a logical operation.

causation_id identifies the direct event that produced the current message/event.

sequence provides monotonic ordering per session/channel.

These fields enable reconstruction of:

~~~text
INTENT → TASK → LEASE → ACTION → FAILURE → DIAGNOSIS → REPAIR → VALIDATION
~~~

## 6. Context synchronization

Material task/action messages carry:

- project_epoch
- context_snapshot_id
- state_digest
- applicable requirement/decision/contract hashes

Material changes create a new project epoch. Affected context becomes stale and cannot authorize new material work.

## 7. ACK, NACK and retry

ACK = received.

NACK = rejected with reason and retryability.

Retryable failures include transport loss, queue pressure, timeout, process crash, adapter failure and recoverable internal errors.

Permanent failures include schema errors, unauthorized actions, policy denial, unsupported capabilities, project mismatch, stale epoch/context requiring synchronization, lease ownership errors and secret-policy violations.

Retries are bounded. Dead-letter is explicit.

## 8. Control traffic

Priority order:

1. emergency control
2. synchronization
3. task control
4. failure/recovery
5. council
6. progress/heartbeat
7. bulk

STOP/CANCEL/security blocks cannot be starved by model streaming.

## 9. Persistence

Use transactional outbox:

state mutation + event + outbound record commit atomically.

Use receiver inbox/deduplication:

received message identity is persisted before side effects.

## 10. Recovery

After restart or crash:

1. reconstruct durable state
2. reconcile in-flight delivery
3. inspect outbox/inbox
4. verify agent processes
5. reconcile leases
6. verify workspaces
7. identify stale contexts
8. rehydrate sessions
9. resume only after required gates

## 11. Security

Every message is project-scoped. Secrets are referenced out-of-band and never copied into normal council/log/evidence payloads. Malformed, unauthorized, oversized or cross-project messages are rejected explicitly.


## 12. Canonical registries

The complete message, event and priority enum is maintained in `schemas/mcf-v2/registry.json` and documented in `MCF-V2-REGISTRY.md`.

The material-action chain is not an informal sequence. Each stage has a canonical event type and is persisted/observable through the bus.

No implementation may introduce a message, event, priority lane or transition identifier outside the registry.
