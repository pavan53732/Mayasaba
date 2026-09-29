# MCF-v2 Machine-Readable Contract Package

This document specifies the machine-readable package that will live under schemas/mcf-v2/. It is the canonical schema contract; implementation code must conform to it.

## Package identity

- protocol family: MCF
- major protocol version: 2
- namespace: mayasaba.mcf.v2
- JSON Schema: Draft 2020-12
- normal serialization: JSON
- streaming serialization: NDJSON
- timestamps: RFC3339 UTC
- IDs: UUIDv7 preferred
- content hashes: SHA-256
- schema compatibility is explicit and testable

## Canonical package

~~~text
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
│   ├── valid/
│   ├── invalid/
│   ├── replay/
│   ├── recovery/
│   └── concurrency/
└── conformance/
    ├── protocol-conformance.yaml
    └── adapter-capabilities.yaml
~~~

## Canonical registries

There is exactly one:

- primitive/type registry
- enum registry
- envelope definition
- message-type registry
- event-type registry
- transition registry

Every message type maps to exactly one payload schema.

## Canonical message rules

ACK means receipt.

TASK_ACCEPT means ownership acceptance.

IMPLEMENTATION_REPORT is an agent report.

VALIDATION is an objective validation result.

CERTIFICATION is controller-owned only.

No generic untyped command message exists.

## Canonical context contract

A ContextPack contains objective, requirements, acceptance criteria, HARD_LOCKs, relevant decisions/assumptions, architecture/contracts, task/dependencies, workspace scope, relevant files/evidence, recent failures/repairs, validation requirements, policy constraints, project_epoch, snapshot ID and state digest.

## Canonical task lease

A TaskLease contains task/owner/session/workspace identity, project epoch, ContextPack identity, allowed paths/capabilities, expiry/heartbeat rules, validation requirements and lease version.

## Canonical handoff

A HandoffPacket contains completed/pending work, changed files, diff/checkpoint/commit references, tests, failures, evidence, affected decisions/contracts, risks and current context/state digest.

## Transition contract

Every legal transition is machine-described by:

- from_state
- event_type
- to_state
- guards
- authorization
- required_fields
- forbidden_fields
- side_effects
- emitted_events
- idempotency requirement
- transaction boundary
- failure transition

## Versioning

Within v2, additive optional fields are allowed. Breaking type/semantic changes require a major protocol version or an explicit migration contract.

## Package status

The package contract is specified here. The actual JSON Schema files, fixtures and conformance harness are an implementation milestone and must be created under schemas/mcf-v2/ before MCF-v2 can be declared implemented.


Implementation mechanics, durable inbox/outbox behavior, ordering, replay and delivery recovery are defined separately in `MCF-V2-IMPLEMENTATION-DESIGN.md`.