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

Each of the three supported adapters must pass the same behavioral contract:

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

## Project intent and user-contribution tests

Must prove:

1. the `ProjectBrief` version current at DISCOVERY close is the anchor recorded in the resulting snapshots and council rounds, and a later brief version does not alter those records;
2. each of the three materiality outcomes produces the correct epoch effect — a material truth change increments the epoch and invalidates affected contexts, a non-material clarification creates a new snapshot/digest at the current epoch, and commentary changes nothing;
3. an advisory classification cannot itself cause or avoid an epoch transition: a contribution classified as a clarification that reveals a material constraint increments the epoch only when the owning service accepts the change, and one classified as a requirement that is already satisfied does not;
4. a free-text contribution cannot reach CouncilService, DecisionService or LifecycleService except through the existing mediated commands;
5. DISCOVERY and USER_INTERVIEW are not both entered for the same clarification, and USER_INTERVIEW is reached only from a deliberation-produced question.

## Workspace tests

- concurrent Git worktree isolation for software repositories
- integration conflict handling
- non-Git checkpointing for file-focused tasks
- user-selected workspace and task-path enforcement
- no whole-PC scan by default
- outside-scope escalation
- process cleanup
- preview and recovery behavior for bulk/destructive data edits
- an `ADMITTED` admission record cannot carry a `FAIL` check, and a `REFUSED` record carries at least one reason
- a lease is never issued against a non-`ADMITTED` `WORKSPACE_ADMISSION`
- an integration whose base checkpoint no longer matches the workspace is `REFUSED` rather than integrated
- a re-evaluation supersedes the prior admission instead of rewriting it
- a changeset with no `ADMITTED` `INTEGRATION_ADMISSION` blocks certification

## Software-engineering loop tests

At minimum, exercise:

idea → requirements → architecture → task → implementation → integration → build → test → E2E → review → failure → diagnosis → repair → regression → certification.

## Local artifact workflow tests

Exercise representative user-authorized tasks for document/report edits, read-only public-source research with local citations, and data cleanup. Validate task-appropriate acceptance checks (for example, format/schema checks, source metadata, record counts, invariants and recoverability) without imposing irrelevant build/E2E gates. Use fixtures or controlled public-read test inputs; test that retrieval has no authenticated or side-effecting path.

## Council collaboration vertical-slice tests (M4.5)

Use three deterministic simulated participants — one per supported agent under DEC-029 — over the real CouncilService, MCF bus, ContextService and minimal Control Room question surface. No live model/provider is required. The slice must prove:

1. each participant receives the same initial project/round snapshot and submits an independent proposal before seeing peers' proposals;
2. CouncilService persists each position with its author, source message, message type, response targets and evidence references, then assigns critique targets before the critique phase;
3. critiques, rebuttals, revisions, stance messages and agent decision/lock candidates reference valid position IDs; missing, self-referential or out-of-round targets are rejected;
4. equivalent question candidates are deduplicated, evidence-check results are retained, and only material unresolved questions are shown to the user;
5. `answer_user_question` durably records the answer and redistribution scope; affected agents receive the current snapshot/digest through existing MCF-v2 context synchronization and revise against it;
6. an agent holding the previous snapshot is rejected from material work; duplicate and delayed delivery does not duplicate the answer or state change;
7. a transport ACK is not treated as proof of context application, and non-participation/timeouts are visible rather than represented as agreement;
8. agent-sent DECISION/LOCK messages cannot create an authoritative decision or HARD_LOCK without the controller/user decision path.

The test must use only existing MCF-v2 message types and payload schemas. It demonstrates the collaboration and user-answer flow without modifying the MCF-v2 envelope or payload contract.

## Council termination and escalation tests

Must prove:

1. a round advances only while `round_index < max_rounds`; a round at the cap cannot advance and must close or escalate;
2. a round whose position set reaches a fixpoint terminates as `CONVERGED` before the cap, and the fixpoint test is not satisfied by agents agreeing by count;
3. reaching the cap without convergence is recorded as `CAP_REACHED`, and is not conflated with `ESCALATED`;
4. an escalation packet validates against `schemas/council-v1/escalation.schema.json`, its conflict matrix cites only requirement IDs actually in dispute, and its recommendation is never persisted as a decision;
5. a `SYNTHESIS` position is accepted only from the round's temporary chair, cites every contributing position, and is rejected when it merges only the chair's own position;
6. an escalated question that times out remains open, and the timeout is never recorded or rendered as an answer or as assent.

## Council quality tests

These prove the DEC-052 decision-quality rules: that deliberation depth is proportional to risk, that a
convergence rests on warrant rather than agreement, that agreement between two forks of one codebase is not
counted as independent corroboration, that the chair's synthesis is checked by someone other than the chair,
that deliberation is bounded in cost as well as in rounds, and that what a decision later became is recorded
from controller facts alone. They are implemented in `crates/council`.

Must prove:

Modes
1. identical structured inputs produce an identical mode and an identical ordered reason list, so selection is deterministic;
2. every input that contributed appears in the reason list, and a threshold change supplied as configuration changes the outcome — the thresholds are not constants in code;
3. an agent-supplied mode is ignored: no API in the crate accepts a mode from a caller other than the controller, and a caller cannot downgrade a computed mode;
4. a user override is recorded with `override_source = USER`, appears in the reason list, and may raise **or lower** rigor, because the user is the authority (DEC-013); a downward override is recorded, never refused and never silent;
5. escalation is upward only and only between rounds — repeated validation failure or a reviewer `BLOCK` moves `SOLO → REVIEW → FULL` — and there is no path that lowers a mode mid-round;
6. `SOLO` persists a `ModeSelection` record and opens no round, so a SOLO decision point has a record whose round reference is null;
7. a mode changes no state path: no mode adds, removes or reorders a `council_round` state, edge, command or guard.

Evidence
8. a load-bearing claim whose references do not resolve grades `ASSUMPTION`;
9. an agent-supplied grade is not accepted, and the grading API has no parameter through which a caller could supply one;
10. a position's grade is the weakest grade among its load-bearing claims, and a position with no load-bearing claims is handled explicitly rather than defaulted;
11. a round on a material decision may not seal `CONVERGED` while a surviving position carries a load-bearing `ASSUMPTION` claim; the fixpoint predicate is unchanged and no `outcome_type` value is added;
12. Kilo Code and OpenCode count as **one** corroboration because they share a lineage group, and Hermes with either counts as **two**; fewer than two lineage groups is reported as `uncorroborated`;
13. lineage is taken from static adapter facts and never from an agent's own report.

Roles
14. role assignment is deterministic and rotates across rounds, so one agent is not always the skeptic;
15. the skeptic is drawn from a different lineage group than the leading proposal's author when possible, and `LINEAGE_UNAVOIDABLE` is recorded in the assigned reason when it is not;
16. a participant that does not perform its assigned duty is recorded as non-participation for that duty;
17. a role grants no authority: nothing in the role path authorizes a material action or a decision.

Synthesis
18. a synthesis that omits a surviving position is detected and the omitted position ids are reported;
19. a synthesis reviewer must be a non-chair participant who is not the synthesis author, so the chair can never self-certify;
20. the review uses the existing `CRITIQUE` message type targeting the synthesis position id, and adds no message type.

Budgets
21. exhausting any budget produces a documented existing outcome (`ESCALATED` or `CAP_REACHED`) and never silent acceptance;
22. an adapter that reports no token usage yields `UNAVAILABLE` with no amount, and no figure is invented or estimated;
23. time spent paused — an offline agent, or `USER_INPUT_REQUIRED` — is excluded from the wall-clock budget and still recorded, and a round with pauses is not treated as having consumed them.

Outcomes
24. outcome records are append-only: no update or delete path exists, and a superseding record is appended instead;
25. an outcome links only to an existing decision, and `HELD` is refused unless it carries a validation evidence reference;
26. a reversal is produced by the `reopen_decision` command and a supersession by decision supersession, never by an agent claim;
27. mode selection output is byte-identical with and without outcome data present, proving outcome tracking cannot influence routing, thresholds or mode selection;
28. reporting shows raw counts with the sample size per agent and per decision class, and shows no percentage below the configured minimum sample.

Storage
29. opening a database created from the previous `schema.sql` adds the six new tables idempotently and leaves existing data intact, and opening twice is safe;
30. the DDL's `CHECK` vocabularies and the contract vocabularies are the same sets, so a value the contract permits is not refused by the database and a value it forbids is not stored.

Contracts
31. `npm run verify:contracts` passes with every new council schema invariant-checked rather than parse-only, and `npm run codegen` produces no diff.

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

The local contract gate — `npm run verify:contracts`, which runs `tools/contracts/verify.mjs` from the repository root on the user's Windows PC — must compare:

- `schemas/mcf-v2/registry.json` against protocol/message/event enums — performed by the gate;
- `schemas/tauri-bridge-v1/bridge.schema.json` bridge identifiers against their declared owners in `workspace.manifest.json` — performed by the gate. The gate compares the identifier **enums** against the manifest's ownership maps; it does not read the generated `bridge.ts`/`bridge.rs` files or compare them for regeneration drift, which is not yet implemented (see `INTERNAL-APPLICATION-ARCHITECTURE.md` §29);
- `docs/WORKSPACE-MANIFEST.md` against Cargo/npm/Tauri manifests once implementation exists — **partially** performed: the gate currently checks that the declared manifests exist and that each crate's `Cargo.toml` names its declared `mayasaba-*` dependencies. It does not yet parse the root `Cargo.toml`, desktop `package.json` or `tauri.conf.json` contents for a full dependency-graph comparison;
- adapter probe results against the adapter capability contract — **not** performed by the gate. This is a runtime comparison against a live probe result; the gate checks the adapter set, transports and declared controls statically.

Any mismatch the gate detects is a verification failure, not a warning.

> **Operational note:** the gate is a local command, and it is wired to a pre-commit hook rather than run by a service. `git config core.hooksPath .githooks` enables it, but that is a per-clone opt-in that git cannot enforce, so a fresh clone commits without the gate until it is set, and `git commit --no-verify` bypasses it on any commit. When the pointer is unset the verifier prints the fact in its summary line — `Pre-commit gate: NOT enabled (core.hooksPath unset)` — as information rather than a failure, so a deliberate opt-out is visible and is not itself treated as an error.

There is no hosted CI; see the DEC-036 record in `docs/DECISION-REGISTER.md`. Running the gate before handoff and commit is required, not optional.
