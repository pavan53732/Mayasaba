# Mayasaba Conformance and Testing Strategy

## Purpose

Mayasaba's autonomy is accepted only when its coordination, software-engineering behavior, local-artifact workflows and action boundaries are testable.

## Protocol conformance

Every MCF-v2 implementation must test:

- schema validity
- message-to-payload mapping
- enum correctness
- channel compatibility
- transition legality
- required guards
- ACK/NACK
- retry
- idempotency
- ordering
- sequence gaps
- correlation/causation
- dead letters
- replay

## Adapter conformance

Each of the four initial adapters must pass the same behavioral contract:

- discovery
- version
- handshake
- capability negotiation
- workspace validation
- ready state
- send/receive
- streaming
- interrupt
- resume where supported
- stop
- process-state verification
- change/evidence collection

Native capabilities may differ, but unsupported features must be reported explicitly.

## Recovery tests

Must simulate:

- CLI crash
- adapter crash
- Mayasaba restart
- UI restart
- duplicate delivery
- delayed delivery
- out-of-order delivery
- crash after inbox persistence
- crash after side effect before acknowledgement
- outbox not dispatched
- lease expiry
- stale context
- epoch change
- partial council participation

## Workspace tests

- concurrent Git worktree isolation for software repositories
- integration conflict handling
- non-Git checkpointing for file-focused tasks
- user-selected workspace and task-path enforcement
- no whole-PC scan by default
- outside-scope escalation
- process cleanup
- preview and recovery behavior for bulk/destructive data edits

## Software-engineering loop tests

At minimum, exercise:

idea → requirements → architecture → task → implementation → integration → build → test → E2E → review → failure → diagnosis → repair → regression → certification.

## Local artifact workflow tests

Exercise representative user-authorized tasks for document/report edits, read-only public-source research with local citations, and data cleanup. Validate task-appropriate acceptance checks (for example, format/schema checks, source metadata, record counts, invariants and recoverability) without imposing irrelevant build/E2E gates. Use fixtures or controlled public-read test inputs; test that retrieval has no authenticated or side-effecting path.

## Council collaboration vertical-slice tests (M4.5)

Use four deterministic simulated participants over the real CouncilService, MCF bus, ContextService and minimal Control Room question surface. No live model/provider is required. The slice must prove:

1. each participant receives the same initial project/round snapshot and submits an independent proposal before seeing peers' proposals;
2. CouncilService persists each position with its author, source message, message type, response targets and evidence references, then assigns critique targets before the critique phase;
3. critiques, rebuttals, revisions, stance messages and agent decision/lock candidates reference valid position IDs; missing, self-referential or out-of-round targets are rejected;
4. equivalent question candidates are deduplicated, evidence-check results are retained, and only material unresolved questions are shown to the user;
5. `answer_user_question` durably records the answer and redistribution scope; affected agents receive the current snapshot/digest through existing MCF-v2 context synchronization and revise against it;
6. an agent holding the previous snapshot is rejected from material work; duplicate and delayed delivery does not duplicate the answer or state change;
7. a transport ACK is not treated as proof of context application, and non-participation/timeouts are visible rather than represented as agreement;
8. agent-sent DECISION/LOCK messages cannot create an authoritative decision or HARD_LOCK without the controller/user decision path.

The test must use only existing MCF-v2 message types and payload schemas. It demonstrates the collaboration and user-answer flow without modifying the MCF-v2 envelope or payload contract.

## Anti-hallucination tests

The system must prove that:

- agent completion text does not certify a task
- nonexistent commands are not invented when repository facts disagree
- tests cannot be deleted/disabled to force a pass without an explicit reviewed decision
- HARD_LOCKs cannot be silently bypassed
- stale context cannot authorize writes
- expired leases cannot authorize writes
- conflicting agent claims trigger resolution rather than silent selection

## Security tests

- cross-project message rejection
- secret leakage detection
- policy denial
- unauthorized tool invocation
- destructive/admin escalation
- hidden command detection
- denial of out-of-scope file access/writes without approval
- no whole-PC scan by default
- public-web research remains read-only and user-requested
- external side-effect attempts are denied even through native agent tools
- no general control of unrelated applications

## Release gate

A Mayasaba release must have passing conformance results for protocol, adapters, storage, recovery, workspace scope, local artifact workflows, execution policy, validation and certification. Failed mandatory suites—including external-action boundary tests—block release.


## M0.5 integration-seam tests

Before any real adapter is considered integrated, the repository must pass a deterministic seam suite covering:

1. workspace manifest/dependency graph validation;
2. MCF registry loading and enum validation;
3. MCF envelope encode/decode with one valid and one invalid fixture;
4. bus inbox/outbox persistence using a simulated adapter;
5. Tauri command/query identifier validation;
6. Rust→Tauri event emission and React-side event decoding using the bridge schema;
7. empty-schema UI↔Rust round-trip with correlation ID preservation;
8. Windows process cancellation/cleanup smoke test;
9. runtime agent probe result schema validation.

This suite belongs to M0.5 and must run before M1/M2/M3 vertical integration. It does not require any real AI provider or live agent.

## Cross-contract drift tests

CI must compare:

- `schemas/mcf-v2/registry.json` against protocol/message/event enums;
- `schemas/tauri-bridge-v1/bridge.schema.json` against generated Rust and TypeScript bridge identifiers;
- `docs/WORKSPACE-MANIFEST.md` against Cargo/npm/Tauri manifests once implementation exists;
- adapter probe results against the adapter capability contract.

Any mismatch is a build failure, not a warning.
