# MCF-v2 Protocol Contract

MCF-v2 is the canonical Mayasaba communication contract.

## 1. Delivery model

At-least-once delivery with deterministic idempotency.

```text
CREATED → PERSISTED → QUEUED → DISPATCHED → RECEIVED → ACKED → PROCESSING → PROCESSED
```

Failure paths include retry, expiry, rejection, and dead-letter.

ACK means receipt only.

## 2. Canonical envelope

Every message contains:

- protocol_version
- schema_version
- message_id
- event_id
- project_id
- session_id
- sender
- recipients
- channel
- message_type
- phase
- correlation_id
- causation_id when applicable
- sequence
- project_epoch
- context_snapshot_id when required
- state_digest when required
- priority
- created_at
- optional expires_at
- requires_ack
- requires_response
- blocking
- idempotency_key when side-effecting
- typed payload
- references
- security metadata

## 3. Core message families

Lifecycle/control:

HANDSHAKE, HANDSHAKE_ACK, READY, HEARTBEAT, ACK, NACK, PAUSE, RESUME, CANCEL, STOP, ERROR, RETRY, DEAD_LETTER

Synchronization:

SYNC_REQUEST, SYNC_RESPONSE, STATE_DIGEST, CONTEXT_UPDATE, STALE_CONTEXT, EPOCH_CHANGED

Council:

IDEA, PROPOSAL, QUESTION, CRITIQUE, COUNTERARGUMENT, REBUTTAL, REVISION, AGREE, DISAGREE, BLOCK, ACCEPT, REJECT, ABSTAIN, DECISION, LOCK

Task:

TASK, TASK_ACCEPT, TASK_REJECT, TASK_LEASE, TASK_LEASE_RENEW, TASK_RELEASE, TASK_PROGRESS, HANDOFF_REQUEST, HANDOFF_ACCEPT, HANDOFF_REJECT

Engineering:

IMPLEMENTATION_REPORT, FAILURE, DIAGNOSIS, REPAIR_REQUEST, REPAIR_RESULT, REVIEW, TEST_RESULT, VALIDATION, CERTIFICATION

Execution/evidence:

EXECUTION_REQUEST, EXECUTION_STARTED, EXECUTION_RESULT, ARTIFACT_PUBLISHED, EVIDENCE_PUBLISHED

## 4. Strong invariants

1. No unmanaged direct agent-to-agent channel.
2. ACK never means success.
3. Agent completion claims are never authoritative.
4. No material action without valid identity, workspace, policy, lease, current context, and relevant contract/decision.
5. Stale context cannot execute current material work.
6. Duplicate messages cannot cause duplicate material side effects.
7. Expired leases cannot authorize writes.
8. User interruption has operational priority.
9. State transitions are durable and replayable.
10. Historical events are immutable.
11. One subsystem owns each protocol/state concept.
12. COMPLETE is controller-certified.

## 5. Context synchronization

Every material task/action includes a project_epoch, context_snapshot_id, state_digest and relevant requirement/decision/contract hashes.

Material authoritative changes increment project_epoch and invalidate affected contexts.

## 6. Task leases

Task ownership is explicit and time-bounded. A lease contains task, owner session, workspace, epoch, context, allowed paths/capabilities, expiry, heartbeat and validation requirements.

## 7. Handoffs

Handoffs carry objective, completed work, pending work, files/diffs, checkpoints/commits, tests, failures, evidence, decisions, contracts, risks and current context.

## 8. Recovery

Mayasaba restart and agent crash recovery use durable events, outbox/inbox records, lease reconciliation, workspace verification, context rehydration and adapter health validation.

## 9. Persistence pattern

Use transactional outbox:

state mutation + event + outbound record commit atomically, then dispatch asynchronously.

Use receiver inbox/deduplication:

record receipt/idempotency before a side effect and make duplicate delivery harmless.
