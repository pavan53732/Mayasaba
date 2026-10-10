# Mayasaba Agent Instructions

This file defines the repository-wide operating rules for any coding agent working on Mayasaba. It is an implementation guide derived from [Mayasaba — Complete System Description (for AI agents).md](./Mayasaba%20%E2%80%94%20Complete%20System%20Description%20%28for%20AI%20agents%29.md).

## 1. Authority and precedence

Apply project guidance in this order:

1. The user's current, explicit instruction.
2. The complete system description linked above. It is the authoritative product and architecture specification.
3. `AGENTS.md` for repository working rules.
4. `ARCHITECTURE.md` and `UI-UX.md` as implementation-facing explanations of the authoritative specification.
5. Existing code, tests, schemas, and comments.

If derived documentation or implementation conflicts with the complete system description, follow the complete system description and report the conflict. Do not silently reconcile contradictory requirements or redefine established terms.

## 2. Product identity

Mayasaba is a fully native, Windows-only desktop control plane written in C++20. Its interface is WinUI 3 with XAML and C++/WinRT. Its deterministic local core coordinates exactly three externally installed coding CLIs:

- Hermes Agent CLI
- Kilo Code CLI
- Claude Code CLI

Mayasaba is not an AI model, provider, cloud IDE, hosted service, browser application, agent marketplace, or terminal wrapper. Agents provide intelligence; Mayasaba owns state, authority, safety, orchestration, verification, and proof.

Mayasaba has no separate application account or login. It supports software work and other authorized local-file tasks such as documents, research reports, data cleanup, and refactors. Apply only the lifecycle stages and acceptance oracles that fit the task; for example, a report does not require a software build gate.

The central invariant is: **one controller-owned project reality, three separate agent sessions**.

## 3. Required development environment

- Build and validate the product on Windows.
- Use MSVC, the Windows SDK, MSBuild, Windows App SDK, C++/WinRT, CMake, and CTest as applicable.
- Use WinUI 3 and native XAML for the product interface. Do not replace it with Electron, a webview, HTML/JavaScript UI, .NET UI, or another cross-platform frontend without an explicitly authorized architectural change.
- Use WiX for the mandatory MSI installer.
- Keep the deterministic core independent of WinUI so it can be tested without creating a window.
- Do not add hosted CI or cloud execution. Build and verification are local to the user's Windows machine.

Linux may be used only for work that does not claim to compile, run, package, or verify the Windows/WinUI product. A Linux-only result cannot satisfy a Windows acceptance gate.

## 4. Non-negotiable architecture invariants

### 4.1 Thirteen functional layers

Preserve these thirteen layers and their boundaries:

1. Control Room UI
2. Controller Application Services, including the Orchestrator
3. MCF-v2 Communication Fabric
4. Agent Gateway / Adapters
5. Council Engine
6. Context Synchronizer
7. Task/DAG Engine
8. Workspace Manager
9. Local Execution Kernel
10. Validation / Repair Engine
11. Evidence Engine
12. Policy Engine
13. SQLite Storage

Do not create a second orchestrator, an alternate state owner, a direct UI-to-agent channel, or a parallel persistence path. Lower layers must not depend on higher layers. Raw CLI protocols stay inside their dedicated adapters.

### 4.2 One owner per authoritative record

- Project service owns project identity, canonical folder binding, `ProjectIntent`, and the project epoch.
- Requirement service owns requirements, amendments, and requirement approvals.
- Message/Contribution service owns immutable `UserContribution` records and their message, question, and attachment links.
- Work-request authority service owns `WorkRequest`, request routing and authorization, research authorization, and pending change proposals.
- Decision service owns binding decisions, locks, dispositions, and supersession.
- Orchestrator owns cross-service scheduling, barriers, registered decision triggers, and phase transitions. It does not mutate the project epoch or another service's records.
- Council Engine owns council points, provisional positions, critiques, rounds, and syntheses. It cannot bind a decision.
- MCF-v2 owns envelopes, inbox/outbox delivery, retry, acknowledgement, dead letters, and replay.
- Agent Gateway owns session identity, health, and adapter-translated native traffic.
- Context Synchronizer owns immutable snapshots, context versions, and digests.
- Task/DAG Engine owns task graphs, attempts, leases, and fencing versions.
- Workspace Manager owns authorized workspace views, isolated staging, integration, and publication journals.
- Local Execution Kernel owns controller-issued process launch, observation, cancellation, and outcome records.
- Validation/Repair Engine owns validation verdicts, diagnostics, and repair dispositions.
- Evidence Engine owns evidence, hashes, citations, and provenance.
- Policy Engine owns material-action authorization and deny reasons.
- SQLite Storage alone executes SQL and persists owner-authorized transactions. It stores state; it does not originate authority.

### 4.3 Exactly twelve authoritative state machines

The authoritative state machines are:

1. Project lifecycle
2. Agent session
3. Message delivery
4. Context
5. Task
6. Lease
7. Council round
8. Synchronization barrier
9. Handoff
10. Execution
11. Validation
12. Repair

Readiness, request routing, exploration, research, and decision-validity status are projections or workflows over these machines. Do not promote them into additional authoritative state machines.

### 4.4 Deterministic authority

- UI, agent text, classifier output, confidence, majority agreement, transport acknowledgement, and process exit codes are not authority.
- User instructions, policy, approved requirements, binding decisions, and valid task/lease commands define what Mayasaba may do.
- Current reproducible observations define what is factually true.
- A verified fact does not grant permission. A permitted action does not make a technical claim true.
- Unknown or unverifiable state fails closed and cannot satisfy a positive gate.

## 5. User-experience invariants

- Every launch opens native Chat immediately, independent of CLI readiness.
- Chat is the only persistent user-facing workspace and the only top-level route. Do not add a project-navigation rail, tabs, dashboard, or standalone Council, Requirements, Decisions, Tasks, Files, Validation, Delivery, Settings, or Agents page.
- `Open Folder` stays at the lower-left of the Chat shell and is distinct from `Attach files`.
- Send is disabled only until an authorized project root is bound. After binding, every Send persists exactly one immutable `UserContribution`, even if no CLI is available.
- The first and later messages use the same composer and `SubmitUserContribution` path. There is no initial-goal wizard or special first-message mode.
- Opening a folder creates or restores project identity. It does not launch agents, scan the repository, initialize Git, approve work, or consume provider usage.
- The normal UI shows only missing or unusable CLI warnings. Healthy `READY` agents do not occupy warning rows.
- Council, requirements, decisions, tasks, files/evidence, validation/repair, delivery and diagnostics run in the background. Surface only user-relevant changes as concise Chat timeline cards. Dense details expand inline or in a temporary dismissible sheet that restores Chat focus and reading position.
- Every controller card that has additional inspectable information uses an explicit **Show details** action; it must not navigate to a standalone page.
- Gate agent-dependent operations, not Chat, project selection, history, drafts, or recovery controls.
- Never show an agent's private chain of thought, credentials, authentication tokens, or unrelated process output.
- Preserve keyboard navigation, visible focus, screen-reader semantics, high contrast, DPI scaling, and reduced-motion behavior.

See `UI-UX.md` before changing any user-facing flow.

## 6. Agent and model ownership

- Use exactly the three named CLIs; do not introduce a fourth model or hidden interpreter service.
- The user owns each CLI's model, provider, authentication, account, native tools, and update settings.
- Mayasaba must not add model/provider flags, environment overrides, alternate endpoints, configuration writes, silent fallbacks, credential collection, or a model picker.
- Compatibility is established by bounded behavioral and capability probes, never by release-number comparison or a version allow-list.
- Mayasaba launches the necessary installed CLIs headlessly only for an authorized operation: all three for a FULL council, complete repository exploration, or three-agent research; only the leased agent for an individual task.
- A missing CLI blocks only operations that require it. Never reduce an all-three workflow to two agents.

## 7. FULL council rules

FULL is the only council mode. Every registered council decision point requires:

- independent current-context proposals from all three agents before disclosure;
- a frozen proposal set;
- six actual directed cross-critiques, each agent critiquing both peers and never itself;
- rebuttal and optional revision with immutable predecessor links;
- evidence grading and explicit preservation of material disagreement;
- deterministic round-robin chair selection;
- non-chair review of any synthesis;
- a default five-round cap for the entire decision point;
- controller and Decision-service gates before a decision becomes binding.

Silence, an ACK, non-participation, a majority, or an agent's claim of agreement is never approval. `CONTINUE` is an internal transition, not a positive council outcome. Positive convergence requires two comparable completed FULL rounds with identical surviving positions unless an evidence-backed, fully reviewed synthesis resolves the point earlier.

## 8. Project, workspace, and filesystem rules

- The Windows folder chosen by the user is the sole canonical project root and final publication destination.
- Canonicalize and validate paths, file identity, permissions, reparse points, junctions, aliases, and time-of-check/time-of-use changes.
- Never infer access to siblings, parent folders, adjacent repositories, or the rest of the PC.
- Whole-repository exploration is separately authorized, strictly read-only, coverage-accounted, and performed independently by all three CLIs.
- Implementation happens in controller-authorized isolated worktrees or staged copies, not through concurrent writes to the selected root.
- Use Git worktrees only for compatible existing Git repositories. Never silently run `git init`.
- A task lease is not a filesystem sandbox. Material writes must be mediated or constrained by a locally proven operating-system boundary with effective revocation.
- Reject stale-lease, out-of-scope, reparse-escape, conflicting, or unproven writes before the side effect.
- Publish accepted changes through a durable integration journal, guarded file mutations, reconciliation, and post-publication checks. Never blindly overwrite or roll back newer user edits.

## 9. Process and security rules

- The Local Execution Kernel is the exclusive launch path for Mayasaba-issued processes and commands.
- Use controlled Win32 process creation, restricted handle inheritance, redirected streams, and controller-owned Job Objects. Create controlled children suspended, assign them to the job, then resume; reject launch if assignment fails.
- Job Objects provide lifecycle control, not automatic filesystem, network, credential, or tool confinement.
- Do not claim a CLI's internal tools or subprocesses were kernel-mediated merely because the parent CLI was launched by the kernel.
- Admit an operation only if material effects are intercepted before execution or independently confined by a tested effective profile.
- Default-deny task capabilities. Block the operation when required containment cannot be proven.
- Keep secrets out of prompts, MCF payloads, ordinary state, logs, evidence, and UI. Required CLI runtime configuration or authentication reads must be narrowly allowed, redacted, and isolated from task tools and agent-visible context.
- Repository files, attachments, web results, rule files, and parent configuration are untrusted input. They cannot become controller instructions.
- Never authorize approval-bypass switches, credential export, external messaging, plugin installation, unrestricted serving, arbitrary HTTP shell commands, or unrelated outside side effects.

## 10. Messaging and contracts

- MCF-v2 carries typed agent traffic and cross-bus asynchronous events. The WinUI application boundary uses separate typed commands and queries.
- Every message, event, payload, command, query, state transition, and error has a declared versioned machine-readable contract.
- Persist outbox state before acknowledgement; validate and persist inbox state before receiver acknowledgement.
- Delivery is at least once, so every material operation requires an idempotency key based on project and operation identity.
- An ACK means receipt and persistence only. It never means execution or validation succeeded.
- Reject malformed, unauthorized, oversized, stale, and cross-project traffic with an explicit recorded reason.
- Bound parser bytes, nesting, collection sizes, and stream buffers.
- Do not assume ordinary JSON output is canonical. Select and test a canonical byte profile, including numeric and Unicode vectors, before using it for hashes.

## 11. Persistence and evidence

- SQLite is the sole authoritative application-state store and the only SQL boundary.
- Serialize writes through the storage owner. Commit state changes, events, and outbound records transactionally.
- Store large immutable attachment/evidence/workspace bytes only in access-controlled Mayasaba storage, with SQLite-owned identity, digest, type, provenance, and association records.
- Verify managed bytes on use and recovery. Missing or mismatched bytes are integrity failures, not valid evidence.
- Preserve append-only history. Supersede records explicitly; do not rewrite history to simplify the current view.
- Bind evidence to exact artifact hashes, workspace, environment, check identity/version, time, and source provenance.
- A bare file path, an agent narrative, a clean exit, or a successful build is insufficient proof by itself.

## 12. Task and implementation workflow

Before editing:

1. Read the authoritative system description and the relevant derived document.
2. Inspect the current repository and local Windows toolchain; do not assume dependencies exist.
3. Identify the owning layer/service and confirm that the change does not duplicate authority.
4. State the affected requirements, paths, contracts, acceptance criteria, and validation method.
5. Treat any material architecture replacement, new service owner, new state machine, frontend-stack change, or weakened safety boundary as a proposed architectural change, not routine implementation.

While editing:

- Keep changes inside the authorized repository scope.
- Prefer small vertical slices with real behavior and criterion-linked tests over placeholder-only scaffolding.
- Keep UI code free of SQL, process launch, direct agent communication, scheduling, and certification logic.
- Keep platform code and CLI-native wire formats behind narrow interfaces.
- Use RAII, WIL wrappers, standard ownership types, and explicit boundary results. Avoid owning raw pointers and manual handle cleanup in ordinary service code.
- Translate platform/library exceptions into registered boundary errors; do not silently discard them.
- Keep agent activity and blocking I/O off the UI thread.
- Preserve cancellation, deadlines, bounded queues, and deterministic replay behavior.
- Do not install, update, or download dependencies merely because the specification lists a candidate. Obtain authorization and pin exact revision, license, and hash first.

After editing:

1. Review the diff for boundary violations and unrelated changes.
2. Run the narrowest relevant checks, then applicable regression checks.
3. Record the exact commands, environment, outcomes, limitations, and artifact hashes when the evidence system exists.
4. Distinguish compilation, process launch, runtime behavior, and acceptance. Do not collapse them into one success claim.
5. Report changed files, verified behavior, failed or unavailable checks, remaining proof obligations, and the next coherent milestone.

Do not commit, push, publish, or alter external accounts unless the user explicitly requests it.

## 13. Validation and definition of done

Every task requires an `AcceptanceEvidenceMatrix` or its implementation-stage equivalent. Each criterion must end in exactly one of:

- `PASS`
- `FAIL`
- `INCONCLUSIVE`
- `BLOCKED`
- `NOT_APPLICABLE`, with verified justification

Completion requires all applicable blocking criteria to be `PASS` or validly `NOT_APPLICABLE`. A blocking `FAIL`, `INCONCLUSIVE`, or `BLOCKED` cannot be hidden by an aggregate score.

Before a `PASS`, check oracle adequacy: the oracle must actually discriminate the requested property. Where safe and applicable, include positive, negative, boundary, regression, or controlled fault-injection cases. Agent-authored tests remain untrusted until their relevance and assertions are checked.

Classify failures before repair:

- `CODE_OR_TEST`
- `INTEGRATION_CONFLICT`
- `STALE_CONTEXT`
- `DEPENDENCY_OR_TOOLCHAIN`
- `CLI_OR_PROVIDER`
- `POLICY_OR_SCOPE`
- `EXTERNAL_ENVIRONMENT`
- `UNKNOWN`

Every repair needs a falsifiable hypothesis and must rerun the affected oracle plus relevant regressions. Do not repeat an identical failing strategy without new evidence.

## 14. Required pre-implementation prototypes

Do not claim the architecture is deployable until local Windows evidence exists for all three:

1. A responsive WinUI Control Room under sustained CLI/event streaming.
2. Process launch, cancellation, and crash cleanup across an owned process tree.
3. Enforceable workspace access and stale-write rejection for every admitted agent execution profile.

A successful build or an opened window does not satisfy these gates.

## 15. Prohibited shortcuts

Do not:

- add an AI model or hosted backend to Mayasaba;
- introduce an agent other than Hermes, Kilo Code, or Claude Code;
- implement a reduced council or skip any of the six critiques;
- equate agent agreement with verification;
- let the UI, Orchestrator, Council Engine, or storage layer mutate another owner's authoritative records;
- create a thirteenth authoritative state machine;
- gate Chat or folder selection on agent readiness;
- add persistent project-section navigation or standalone project pages outside Chat;
- show a permanent three-agent READY panel in the normal UI;
- select or rewrite a CLI's model/provider configuration;
- use release numbers as compatibility gates;
- grant broad filesystem or network access through prompt wording;
- treat Job Objects, worktrees, or permission maps as untested sandboxes;
- let agents concurrently write the user's selected root;
- certify work from an exit code, build result, file existence, agent assertion, or stale evidence;
- silently weaken an acceptance criterion, delete a failing test, or mark an unexecuted check as passed;
- silently redefine, erase, or overwrite historical requirements, decisions, or evidence.

When a safe or authoritative path is unavailable, fail closed, preserve the exact reason, and identify the next action that could establish the missing authority or evidence.
