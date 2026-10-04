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

## Durable event chain tests (DEC-034)

The chain is only worth having if it is recomputed from what was actually persisted, so these tests read the
stored rows rather than trusting the writer's return value. `crates/storage/tests/event_chain.rs` runs them
through a real database; `crates/storage/src/chain.rs` runs the same rule against synthetic rows.

Must prove:

- a project's genesis link names the 64-zero `prev_hash`, and its stored `event_hash` equals the hash the
  declared rule recomputes from the stored row — not merely a value written alongside it;
- each appended event names the previous link's hash, so the chain is a chain and not a run of independent
  hashes;
- a mutated field breaks that event's link and every later link, because each link is compared against the
  recomputed hash of its predecessor;
- a deleted event breaks the chain at its successor;
- a changed `sequence` breaks the chain; a physically reordered row does not, because the chain order is
  derived from `sequence` rather than from storage order;
- two projects chain independently, and a project and a session that share an identifier stay separate chains;
- an event carrying no `sequence` is reported as unpositioned rather than silently placed in the chain;
- an event naming neither a project nor a session is refused and persists nothing;
- a startup recovery scan reports a broken chain as `EVENT_CHAIN_BROKEN`, so a tampered database cannot report
  itself clean;
- a failed append leaves the chain exactly as it was.

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
- `schemas/tauri-bridge-v1/bridge.schema.json` bridge identifiers against their declared owners in `workspace.manifest.json` — performed by the gate. The gate compares the identifier **enums** against the manifest's ownership maps. It also reads **both sides** of the bridge (DEC-053): the `#[tauri::command]` functions and the `generate_handler![...]` list in `apps/desktop/src-tauri/src/main.rs`, and the `transport(...)` call sites in the non-generated, non-test frontend. A handler or a call that names an operation the contract does not declare is a failure, as is a call to a declared operation that no handler registers, a `#[tauri::command]` function that is never registered, a registered name that is not a command function, an `invoke(...)` call outside the single transport boundary, a `transport(...)` name that is not a string literal, and a registration list the gate cannot parse. A declared operation that nothing implements is **reported** with its count rather than failed, because the contract deliberately leads implementation — see the mutation proofs below. The gate also compares each handler's **arguments** with the operation's declared `request_fields` (DEC-056): it derives the wire name of every argument from the command's `rename_all`, modelling Tauri's camelCase default rather than assuming it away. An argument the contract does not declare is a failure, a required declared field the handler cannot receive is a failure, and a declared optional field the handler does not accept is **reported** — the same asymmetry applied to whole operations. A `rename_all` convention the gate cannot model is a failure rather than a skip, because an unmodelled convention means the argument names are unchecked;
- `docs/WORKSPACE-MANIFEST.md` against Cargo/npm/Tauri manifests once implementation exists — **partially** performed: the gate currently checks that the declared manifests exist and that each crate's `Cargo.toml` names its declared `mayasaba-*` dependencies. It does not yet parse the root `Cargo.toml`, desktop `package.json` or `tauri.conf.json` contents for a full dependency-graph comparison;
- `schemas/error-v1/registry.json` against the protocol's own error vocabulary and against the code that produces errors — performed by the gate (DEC-055). Every non-null `mcf_code` must be a member of the closed code enum in `schemas/mcf-v2/error.schema.json`; every `category`, `severity` and `retryability` must be a member of its enum there; `mcf_code` and `tauri_code` must each be unique; every entry must state a non-blank `meaning`; and every error code the implementation can produce must be registered. The implementation's codes are collected from a `code: "..."` field across `crates/`, `apps/desktop/src/` and `apps/desktop/src-tauri/src/`, plus the `=> "..."` arms of the two mappings that turn a domain rejection into a code — `WorkspaceRejection::code()` and `From<ProjectValidationError> for CommandError`. Both mapping sites are located by signature and the check **fails closed** if either cannot be found. The gate also reports, without failing, that no `tauri_code` value is emitted anywhere: the wire carries the canonical registry key, and whether it should carry the namespaced spelling instead is an open question recorded in DEC-055;
- `schemas/tauri-bridge-v1/payload-types.json` against the bytes the wire actually carries — performed by the shell's own test suite (DEC-056). `apps/desktop/src-tauri/src/main.rs` contains `mod wire_shape_tests`, which serializes each implemented operation's real response struct with serde and validates it against the real declaration, read from the contract with `include_str!` so the test compares bytes against the file rather than against a copy of it. The validator implements the JSON Schema subset those types use and **fails on any keyword it does not implement**, so the contract cannot start using a constraint that would go unchecked — under-validation that reports success is indistinguishable from a check that passed. `every_registered_handler_is_covered` reads the file's own `generate_handler![...]` list and fails when a registered handler has no shape test or when a covered operation is not registered, so adding a fifth handler without checking its payload is a test failure rather than an omission. This is the check that would have caught the casing divergence (DEC-053, fixed by DEC-054) automatically: it compares the serialized key set with the declared property set, so a renamed, added, removed or recased field fails by name;
- adapter probe results against the adapter capability contract — **not** performed by the gate. This is a runtime comparison against a live probe result; the gate checks the adapter set, transports and declared controls statically.

Any mismatch the gate detects is a verification failure, not a warning.

> **Operational note:** the gate is a local command, and it is wired to a pre-commit hook rather than run by a service. `git config core.hooksPath .githooks` enables it, but that is a per-clone opt-in that git cannot enforce, so a fresh clone commits without the gate until it is set, and `git commit --no-verify` bypasses it on any commit. When the pointer is unset the verifier prints the fact in its summary line — `Pre-commit gate: NOT enabled (core.hooksPath unset)` — as information rather than a failure, so a deliberate opt-out is visible and is not itself treated as an error.

There is no hosted CI; see the DEC-036 record in `docs/DECISION-REGISTER.md`. Running the gate before handoff and commit is required, not optional.

## Mutation tests for the checks themselves (DEC-057)

A check that has never been shown to fail is an assertion, not a check. `npm run verify:contracts:mutations` runs `tools/contracts/mutations.mjs`, which reintroduces the drift each check exists to catch — one mutation at a time, applied to the working tree, run, and restored — and requires the check to fail **and to name the specific disagreement the mutation introduced**. Exiting non-zero is not enough on its own: that would also accept a crash, a mistyped anchor, or an unrelated failure, so each mutation declares the substrings its failure must contain, quoted from the check's own messages. A reworded message fails the mutation suite until the mutation is re-read rather than being matched by something vaguer.

Controls run in the opposite direction and must stay **green**, because a check that fires on prose or on a contract description gets turned off rather than fixed. The unmutated tree is verified green before any mutation, so a mutation's failure is attributable to the mutation.

It is safe to run because it refuses to start when a tracked file is modified — untracked files are permitted, since `git checkout` cannot touch them — and restores every file it touches with `git checkout -- <file>`. It restores on SIGINT and on an uncaught exception as well as normally, and it verifies at the end that the tree is clean and HEAD is unchanged. If it is interrupted in a way that defeats all of that, the recovery is `git checkout -- .`, which is the right command here precisely because the harness refused to start with a modified tracked file.

It is deliberately **not** part of `npm run verify:contracts` and **not** wired to the pre-commit hook: it mutates the tree, and the Rust mutations each pay a recompile. It is a tool to run when a check changes, not on every commit. `--filter <substring>` runs a subset by mutation id.

## Bridge two-way gate mutation proofs (DEC-053)

A check that has never been shown to fail is an assertion, not a check. The two-way bridge gate was therefore mutation-tested in eight directions, each applied to the working tree, run, and reverted. Every mutation made the gate exit non-zero and name the specific disagreement; the unmutated tree exits zero; the tree was byte-identical after each revert. These eight are now mutations `dec053-a` … `dec053-h` in `tools/contracts/mutations.mjs`, so they can be re-run rather than only read (DEC-057).

| # | Mutation | Expected and observed failure |
|---|---|---|
| a | `main.rs`: register an extra `#[tauri::command] fn ghost_undeclared` under a name the contract does not declare | fails, naming `ghost_undeclared` as an operation the contract declares as neither a command nor a query |
| b | `main.rs`: remove `validate_workspace` from `generate_handler![...]` while leaving its function defined | fails: a declared handler is defined but never registered, so it is unreachable from the frontend |
| c | `main.rs`: delete the `#[tauri::command]` attribute from the registered `create_project` | fails: `generate_handler![...]` registers `create_project` but no `#[tauri::command] fn create_project` exists |
| d | `main.rs`: break the `generate_handler![...]` list so it no longer parses | fails closed — "could not find a `generate_handler![...]` list" — rather than skipping the Rust side |
| e | frontend: add `transport("ghost_undeclared_op", {})` | fails, naming the undeclared operation and the file |
| f | frontend: add `transport("get_project", {})`, a declared query with no handler | fails: the call cannot succeed because no handler of that name is registered |
| g | frontend: add a raw `invoke("list_projects", {})` outside the boundary file, and a `transport(dynamicName, {})` with a non-literal name | fails twice: `invoke` outside the single transport boundary, and an operation name that is not a string literal and therefore cannot be checked |
| h | generated surface: edit `apps/desktop/src-tauri/src/generated/bridge.rs` and `apps/desktop/src/generated/bridge.ts` without regenerating | fails on both: `generate-bridge.mjs --check` reports each stale file by name |

Two controls were run alongside them. The unmutated tree passes, and a **commented-out** `transport(...)`/`invoke(...)` call does not fail the gate — comments are stripped before scanning, so a name mentioned in prose is not mistaken for a call site. A gate that fired on comments would be turned off rather than fixed.

What the proofs do **not** establish, stated so the green is not over-read: the gate proves that operation *names* agree, never that a handler's payload matches its declared payload type or that a query is genuinely read-only. Two of those three are no longer true and are named here so the correction is not mistaken for the original claim. The emitted-error-code half was closed by DEC-055, which added a separate check that reads the codes the implementation produces and fails on any the registry does not declare. The wire-casing divergence was fixed by DEC-054, and the payload-shape half was closed by DEC-056 — not by this gate, which still compares only names and argument names, but by a conformance test in the shell that serializes each wire struct and validates it against its declared type. What remains genuinely unproven is the last clause: nothing establishes that a declared query is read-only. All three were reported as open items at DEC-053 rather than implied to be covered, which is why they are named here.

## Wire-shape and handler-argument mutation proofs (DEC-056)

The conformance test and the argument-name check were mutation-tested in eight directions, each applied to the working tree, run, and reverted from a backup copy. Every mutation made the test or the gate exit non-zero and name the specific disagreement; the unmutated tree exits zero. These eight are now mutations `dec056-a` … `dec056-h` in `tools/contracts/mutations.mjs`, where they are restored with git rather than from a copy, so the file's mtime moves forward and cargo cannot mistake a restored file for a fresh one (DEC-057).

| # | Mutation | Expected and observed failure |
|---|---|---|
| a | `main.rs`: `ProjectView` serialized as `camelCase` again — the original defect | the conformance test fails on `create_projectResponse`, naming the missing snake_case keys |
| b | `main.rs`: `RecoveryView.integrity_ok` sent as `integrity` | fails: `get_recovery_statusResponse is missing required key "integrity_ok"` |
| c | `main.rs`: `ProjectView.created_at` marked `#[serde(skip_serializing)]` | fails: `create_projectResponse is missing required key "created_at"` |
| d | `main.rs`: drop `list_projects` from `COVERED_OPERATIONS` | fails: `list_projects is registered in generate_handler![...] but no shape test covers it` |
| e | `main.rs`: name an operation in `COVERED_OPERATIONS` that nothing registers | fails: `COVERED_OPERATIONS names ghost_operation, which generate_handler![...] does not register` |
| f | `main.rs`: `create_project` loses `rename_all = "snake_case"`, so Tauri's camelCase default decides the wire names | the gate fails: `accepts the argument "localPath", which its declared request_fields do not contain (the Rust argument is "local_path"; rename_all decides the wire name)` |
| g | `main.rs`: the handler accepts an argument the contract does not declare | the gate fails, naming the undeclared argument |
| h | `main.rs`: the handler stops accepting a required declared field | the gate fails: `declares the required request field "initial_brief", which the handler does not accept` |

One control was run alongside them: the unmutated tree passes both the test suite and the gate. Mutations (a)–(e) were chosen to **compile**, so a non-zero exit is the conformance test failing rather than the crate failing to build — a mutation that breaks the build proves nothing about the check.

