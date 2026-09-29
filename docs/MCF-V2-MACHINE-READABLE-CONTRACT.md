# MCF-v2 Machine-Readable Contract Package

## Package identity

- Protocol family: MCF
- Major protocol version: 2
- Namespace: mayasaba.mcf.v2
- JSON Schema: Draft 2020-12
- Transport serialization: JSON / NDJSON
- IDs: UUIDv7 preferred
- Content hashes: SHA-256
- Timestamps: RFC3339 UTC
- Schema changes are compatibility-governed

## Canonical package

```text
schemas/mcf-v2/
├── manifest.json
├── common.schema.json
├── enums.schema.json
├── identity.schema.json
├── envelope.schema.json
├── handshake.schema.json
├── ack.schema.json
├── error.schema.json
├── sync.schema.json
├── context-pack.schema.json
├── task.schema.json
├── task-lease.schema.json
├── handoff.schema.json
├── council-round.schema.json
├── barrier.schema.json
├── execution.schema.json
├── artifact.schema.json
├── evidence.schema.json
├── validation.schema.json
├── control.schema.json
├── message-types.schema.json
├── message-payloads.schema.json
├── event-types.schema.json
├── transition-types.json
├── fixtures/
└── conformance/
```

## Canonical registries

- One primitive/type registry
- One enum registry
- One envelope
- One message-type registry
- One event-type registry
- One transition registry

Every message type maps to exactly one payload schema.

## Required message semantics

ACK = receipt.
TASK_ACCEPT = ownership acceptance.
IMPLEMENTATION_REPORT = agent report.
VALIDATION = deterministic result.
CERTIFICATION = controller-owned final result.

## State registries

Agent, message delivery, context, task, handoff, council, execution and validation each have independent state machines.

## Compatibility

Additive optional fields may be added inside v2. Breaking semantic/type changes require a new major version or explicit migration.

## Conformance

Required suites cover schema validation, adapter compatibility, ACK/NACK, idempotency, duplicate delivery, ordering/gaps, stale context, epochs, leases, handoffs, barriers, pause/resume/cancel/stop, crash recovery, replay, dead letters, isolation, secret handling, and compatibility.
