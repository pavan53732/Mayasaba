# Mayasaba — Complete End-to-End Description

Mayasaba is a Windows desktop control plane. A user points it at a folder on their own PC and states a request. Three coding agents deliberate and carry the work out in isolated workspaces, and Mayasaba declares the result done only after it has verified the work itself, with evidence. The agents supply the intelligence; Mayasaba supplies state, authority, safety and proof.

## 1. What Mayasaba is

Mayasaba is the deterministic control plane between a user and three coding agents. The user chooses a local workspace folder and describes the work. Mayasaba plans, coordinates, authorizes, executes and verifies. The agents reason and write.

**The three agents.** Hermes Agent CLI, Kilo Code CLI and OpenCode CLI, and no others. Mayasaba does not replace them or add a model of its own. Each keeps its own model and provider choice, tools, login, session and reasoning. Mayasaba counts independent agreement by **lineage**, not by agent count, because two of the three share a codebase — Kilo Code is a fork of OpenCode — so those two corroborate each other as **one**, never two.

**Supported work.** Software engineering from requirements to a packaged application, and other local-file work: documents, research reports, data cleanup and refactors. A research task may read public web pages, read-only, and saves the report and its citations locally.

**Hard boundaries**

- Windows only. All controlled execution happens on the user's own PC: no cloud machine, hosted workspace or remote executor, and no account or login.
- The folder the user picks, plus task-specific allowed paths, is the filesystem boundary. A typed path is only a candidate until Mayasaba confirms it exists, is local and is allowed. It never scans the whole PC.
- No outside side effects: it does not send messages, publish, submit forms, buy things, change accounts or control unrelated applications.
- It installs from an MSI package and keeps all of its state in a local SQLite database.

**What it refuses to be.** Not a fifth AI model, a cloud IDE, a chat wrapper, an agent marketplace, or a system where an agent has the final word. An agent saying "done" never completes a task.

**Core principle: one project reality, three separate agent sessions.** Mayasaba holds the facts: requirements, decisions, task ownership, context versions, workspace scope, evidence and validation state. The agents never share a hidden brain, and nothing an agent claims becomes true until Mayasaba confirms it.

## 2. The journey, end to end

A software project passes through these stages. Other local-file tasks run only the stages that apply to their acceptance criteria, so a report never needs a build step. A document, research or data task is validated by the checks its own acceptance criteria require — structure, source and citation validity, record counts, invariants or integrity — never by a software gate that does not apply to it.

1. **Intake.** Project creation happens in the Control Room's **Initial Intake Composer** — the text box shown when creating a new project, which is a different surface from the Chat section of the navigation. It takes exactly two required inputs: the **local workspace folder**, chosen with the native Windows folder picker (a typed path is only a candidate until existence, locality and authorization are separately checked), and the **project intent**, free text describing what the user wants Mayasaba to accomplish. Until submission that text is local draft state only. On submission it becomes version 1 of a **project brief**, which is durable project truth — the persisted artifact is the brief record, not a transcript entry, and the chat transcript is never treated as project truth. Attached files are kept as supporting context with their source recorded; attaching a file does not make it truth. The project's display name comes from the workspace folder name, and its identity is a separate opaque id, so two projects with the same folder name never collide.
2. **Discovery.** Mayasaba establishes the brief baseline and objective facts about the workspace: path, repository type, existing files, available toolchain and constraints. It decides whether the brief is complete enough to start. The brief version current at that moment becomes the fixed anchor for the whole analysis.
3. **Independent analysis and proposals.** Every agent receives the same snapshot of the project and analyzes it on its own, without seeing the others, then submits a proposal. This avoids anchoring on whoever speaks first.
4. **Cross-critique, rebuttal and revision.** Mayasaba assigns each agent which proposals to review. Reviewers critique with evidence, authors answer and revise.
5. **Disagreement resolution and user interview.** Material disagreements stay visible. They are settled by evidence, by revision, or by the user. Agents' questions are collected, merged and checked against the workspace first; the user is asked only what is still unresolved, in one batch.
6. **Design and architecture.** Product and UX design, a technology-choice debate and an architecture review end in a **locked architecture**: recorded decisions with rationale, evidence and alternatives.
7. **Task planning.** The work becomes a graph of tasks. Each task has an objective, allowed paths, dependencies, required outputs, acceptance criteria and a validation method.
8. **Implementation.** Agents work on leased tasks, each in an isolated workspace.
9. **Integration, build, test and end-to-end checks.** Mayasaba merges accepted work in a workspace it controls, then builds, tests and exercises the running application. Agents also review each other's work. These are separate results, not one: building successfully, launching a process successfully and behaving correctly at runtime are three different things, and only the last of them is runtime correctness. A process starting up proves nothing about whether the application works.
10. **Repair.** Failures are diagnosed and turned into bounded repair tasks, followed by targeted and regression tests.
11. **Final validation, packaging and certification.** The project is complete only when Mayasaba certifies it from evidence.

The canonical phase sequence beneath these stages — the software-engineering lifecycle — is:

~~~text
PROJECT_CREATED → DISCOVERY → INDEPENDENT_ANALYSIS → PROPOSALS
→ CROSS_CRITIQUE → REBUTTAL_AND_REVISION → DISAGREEMENT_RESOLUTION
→ USER_INTERVIEW → PRODUCT_AND_UX_DESIGN → TECH_STACK_DEBATE
→ ARCHITECTURE_REVIEW → ARCHITECTURE_LOCKED → TASK_PLANNING
→ IMPLEMENTATION → INTEGRATION → BUILD → TEST → E2E
→ CROSS_AGENT_REVIEW → REPAIR (when needed) → FINAL_VALIDATION → PACKAGE → COMPLETE
~~~

`REPAIR` is entered only when required. `COMPLETE` is controller-owned and evidence-backed. Global conditions include `PAUSED`, `STOPPED`, `BLOCKED` and `RECOVERING`; these are orthogonal conditions on the lifecycle, not phases. User interruption has the highest operational priority. A material change to a requirement, a locked decision, the architecture or a contract **invalidates the affected plans**: work already queued under the old plan is recomputed rather than allowed to keep executing it. The Control Room shows every message, conclusion, action, artifact and piece of evidence, never an agent's private chain of thought.

### Where the user actually types

The Control Room is the only user interface, and the chat surface is the user's interface to it: the user types, and typed state transitions — not the transcript — determine what becomes project truth.

There are **two distinct composer surfaces**, named so that intake can never be confused with the Chat navigation section, and so the ongoing chat cannot drift into a second project-creation path:

| Surface | When it appears | What submission creates |
| --- | --- | --- |
| **Initial Intake Composer** | When creating a new project | `ProjectBrief` version 1 — project truth |
| **Ongoing Chat Composer** | After the project exists, in the Chat section | A `UserContribution` with an **advisory** classification |

Both are composer surfaces: **the user types free text into a text box in either case.** What differs is *which surface* the text is entered from and *what gets persisted*. This distinction is easy to misread, so state it carefully:

- **"Not a chat message" describes the artifact, not the input.** When the canonical descriptions say the project intent is "project truth, not a chat message", they mean the stored record is a versioned `ProjectBrief` — not a transcript row that could later be mistaken for authoritative intent. It does not mean the user types it somewhere other than a text box.
- **Intake is not the Chat section.** The navigation sections (Chat, Council, Requirements, …) are all *project* sections, and a project does not exist until intake commits. Intake is therefore the surface shown when no project exists yet; the Ongoing Chat Composer lives inside the Chat section afterwards.

Ownership is split deliberately, so that the transcript is never mistaken for project truth:

| Concern | Owner |
| --- | --- |
| User input interface and draft | Chat composer (draft state only) |
| Project creation | Project service |
| Durable representation of user intent | `ProjectBrief` |
| Structured requirements derived from the brief | Requirement service |
| Analysis context snapshot | Context service |
| Deliberation over the frozen context | Council service |

The composer writes no requirement, decision or epoch directly. Free-text input after creation is recorded as a `UserContribution` carrying an advisory classification, and only the owning authoritative service decides whether project truth actually changed. The Control Room must therefore display the outcome the service produced, not the advisory label: a contribution the service found non-material must never appear to have changed project truth. When a contribution does change truth, the resulting epoch, affected scope and context regeneration are shown; when it does not, it is shown as timeline commentary.

The intake submission surface must also make four states distinguishable, and never invent project state while creation is pending: **draft** (editable local UI state, nothing persisted), **submitting** (command in flight, draft retained, no lifecycle field displayed as authoritative), **created** (the persisted projection returned by the controller), and **rejected** (draft retained alongside a machine-readable error code and message). After commit, the Control Room renders the authoritative persisted projection, never a project reconstructed from the submitted form.

## 3. Technology stack

Mayasaba is a single native Windows desktop application with one Rust core and no hosted component. The stack below is the complete technical surface, described by capability rather than by file.

**Platform**

- Windows desktop, the one target platform.
- Native Windows process and filesystem primitives.
- MSI distribution, as the only way the product is installed.

**Native application**

- Rust as the only language of the core: the controller, protocol, bus, council logic, task engine and every service. Memory safety and a single native binary suit a long-running local control plane.
- Tauri 2 as the desktop shell, so the shell and the authoritative core are the same language and the same runtime and no interop layer sits between them.
- Tokio for concurrent supervision of agent processes, timers, streams and subprocess I/O.
- Windows Job Objects for process control: every agent and tool process is created suspended, assigned to a private job object, and only then resumed, so its whole process tree can be stopped.

**Persistence**

- SQLite, compiled into the binary, as the sole source of truth. Nothing is installed system-wide, so the installer carries no database dependency.
- Transactional persistence: a state change, its event and its outbound record commit together.
- An append-only event history that is never rewritten.
- Integrity hashing over that history, per project, so corruption, deletion and reordering are detectable. The hash chain is not keyed, so it does not resist a deliberate full recompute.

**Serialization and contracts**

- serde and JSON for all messages, contracts and stored payloads.
- Canonical serialization, so a digest over the same content is always the same value.
- JSON Schema as the format of the versioned, machine-readable contract.
- A declared definition for every message type, event, payload, state machine, error code and bridge operation.

**Presentation**

- React 19 and TypeScript, built with Vite.
- Tailwind CSS and shadcn/ui for the visual language: minimal and functional, with a bento-grid layout and restrained frosted-glass accents.
- WebView2, the operating system's own webview engine, as the renderer. Nothing bundles a browser engine, so the installer carries no browser payload and the rendering engine receives security fixes independently of Mayasaba releases.

**Agent integration**

- Hermes Agent CLI, Kilo Code CLI and OpenCode CLI, and no others.
- One adapter per agent, and each adapter is the only place that CLI's native protocol exists.
- Streamed JSON from each CLI's own process as the transport; native formats never leave the adapter.
- Runtime capability probing, so only probe-confirmed facts are admitted as capabilities.

**Execution**

- A single local execution kernel: the only place in the system where a command or process is started.
- Windows process containment, so nothing escapes the controller's lifecycle.
- Workspace authorization: the folder the user selects is the filesystem boundary, and task-scoped allowed paths constrain it further.
- Task leases, so ownership of work is explicit and a stale owner cannot write.
- Isolated workspaces, so concurrent agents do not edit the same working tree.

**Version-controlled engineering**

- Git for version-controlled and worktree-capable work.
- Git worktrees, so each concurrent agent works on its own isolated branch.
- A controller-controlled integration workspace where accepted work is merged.

**Orchestration and control**

- A deterministic orchestrator rather than a model.
- The MCF-v2 communication fabric, which carries all agent traffic.
- The council engine for structured deliberation.
- Context synchronization, versions and digests.
- The task graph and its scheduler.
- Validation and bounded repair.
- The evidence engine behind every claim.
- The policy engine that authorizes every material action.

**Quality and correctness**

- Contract validation, build validation, automated test validation, and runtime and end-to-end validation.
- Evidence-backed certification, which is the only thing that can declare work complete.
- Local-only verification: there is no hosted pipeline, because verification belongs on the user's own machine.

### Why this stack

The choice follows from three constraints, and would change only if one of them changed.

**One runtime.** The controller, the protocol, the bus, the task engine and every service are Rust, and the desktop shell is Rust too. There is no second language runtime and no garbage collector anywhere in the control path, and no interop boundary between the shell and the authoritative core that must be kept in step by hand. Where a shell in another language would put a translation layer between the UI and the truth, there is instead a generated, checkable contract.

**A deliberately thin presentation layer.** The Control Room owns presentation and local interaction state, never project truth. It cannot touch the database, start a process or decide anything; it calls declared operations and renders what the controller returns. The strength of the product therefore does not live in the UI toolkit — it lives in the deterministic Rust layers beneath it, where mode selection, evidence grading, corroboration, fail-closed state machines, lease fencing and certification actually happen. A change of UI toolkit could not make Mayasaba more or less correct, and this is intentional.

**Windows-only makes a system webview safe.** The usual objection to rendering through an operating-system webview is that you must support whatever engine each machine happens to have. That objection does not apply here: there is exactly one target platform, and its webview is evergreen and serviced by the operating system itself rather than by Mayasaba. Bundling a browser engine would add a large payload and a second update channel to buy portability the product does not want.

Two further properties are worth naming. Dense machine-state presentation — an event stream, a context rail, twelve concurrent state machines, evidence chains — is what web rendering and its accessibility model are strongest at, and that model (keyboard navigation, visible focus, screen-reader support) is the most mature available. And the operations that genuinely need to be native **are** native: process containment uses Job Objects, workspace selection uses the native Windows folder flow, and distribution is an MSI.

### What is deliberately absent

No cloud service, no remote database, no hosted component, no second language runtime, no bundled browser engine, and no account. The only network traffic is what each agent CLI itself sends to its own model provider, and user-requested read-only research. The design also uses no hosted build or verification service: everything that checks Mayasaba runs on the user's own machine, because moving execution off that machine would violate the same boundary the product exists to enforce.

## 4. Architecture

The UI talks to Rust application services through a typed bridge. The services drive thirteen layers, and each layer has exactly one owner.

| # | Layer | What it owns |
| --- | --- | --- |
| 1 | Control Room UI | Shows state and collects user decisions. It calls only declared commands and queries; it never touches the database, a process or an agent. |
| 2 | Orchestrator | Phase transitions, scheduling, barriers and gates. It derives the project's overall state from independent state machines. |
| 3 | MCF-v2 Communication Fabric | The message bus: routing, acknowledgement, retry, de-duplication, dead letters and replay. |
| 4 | Agent Gateway / Adapters | One adapter per CLI: detection, probing, launch, streaming, interruption and translation of native output. |
| 5 | Council Engine | Structured deliberation among the agents. |
| 6 | Context Synchronizer | Immutable context snapshots, versions and digests, and stale-context rejection. |
| 7 | Task/DAG Engine | The task graph, leases, attempts, scheduling and recovery of work. |
| 8 | Workspace Manager | Folder boundaries, isolated worktrees, checkpoints and integration. |
| 9 | Local Execution Kernel | The only place a command or process is ever started. |
| 10 | Validation / Repair Engine | Build, test and end-to-end checks, diagnosis and bounded repair. |
| 11 | Evidence Engine | Artifacts, hashes, command records and the proof behind every claim. |
| 12 | Policy Engine | Authorization of every material action. |
| 13 | SQLite Storage | The only component that writes SQL. |

**Ownership rules.** Every layer has one canonical owner and no second copy of its truth. Lower layers never depend on higher ones. The protocol layer depends on no agent or domain implementation. Storage depends on no higher-level layer. The agent layer never lets one adapter depend on another. The orchestrator is the only cross-subsystem orchestration owner. Layers publish typed events across the bus boundary; they do not create ad-hoc callbacks. The Tauri bridge is an application transport, **not** a domain layer. Anything that looks like a "mission", "worker" or "supervisor" is only a view assembled from these records, never a competing source of truth.

**State is never one giant status field.** Twelve independent state machines are each authoritative for one concern:

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

Each has an ordered main path and explicitly declared side branches, and every transition has a defined event and a registered command. The orchestrator reads them and derives the project's overall state.

Three cross-cutting rules govern the state machines: a **cross-machine rule** for how machines interact, a **fail-closed rule** so that unverifiable state cannot satisfy a positive gate, and a **recovery rule**. The whole system separates what Mayasaba intends to happen, what an agent reports and what is physically observed on the machine. Where the physical outcome cannot yet be established, execution and attempt state may be recorded as **unknown** — which is neither a soft failure nor a success: it does not silently consume a retry, and nothing stable and resource-available is allowed to sit running forever unobserved.

**Work is identified separately from its execution.** A task is the stable unit of acceptance and keeps its identity across retries; each retry or reassignment is a separate **attempt** under it. A task is assigned through a **lease**, and the active lease version is the single fencing token for material actions derived from that lease — there is no second fencing authority. Every material side effect must confirm that the acting attempt still holds the current lease version, and a stale one is rejected *before* any side effect occurs. This is what prevents a superseded agent from writing into work that has already moved on.

**What wins when sources disagree**, strongest first:

1. Requirements the user has approved
2. Locked decisions
3. Versioned architecture and contracts
4. Verified facts about the workspace
5. Objective evidence: build, test and end-to-end results, citations and integrity checks
6. Mayasaba's persisted orchestration state
7. Agent proposals and reports

Conflicts between these are recorded explicitly and traced, never resolved silently. Every material requirement is traceable from the user's intent through requirement, decision, architecture, task and attempt, to the evidence, validation and certification that support it; a requirement with no task, work with no owning task, or a certification claim with no evidence is detectable as an orphan.

**Configuration is layered, and the nearest layer wins:** project configuration over user configuration over application defaults. Configuration is versioned and schema-validated, and secrets are referenced rather than copied into ordinary state, messages or logs.

## 5. The agents and the communication fabric

### The agents and their adapters

Every agent is reached through its own adapter, which can detect the CLI, read its version, report capabilities, check health, launch it, send input, stream output, interrupt, resume, stop, and collect changes and evidence. The adapter is the only place a CLI's native protocol exists. What the installed CLI actually does is found by **probing it at runtime**, and only probe-confirmed facts are admitted as capabilities.

| Agent | Transport | Notable controls |
| --- | --- | --- |
| Hermes Agent CLI | Streamed JSON over standard I/O, the only transport | Its default injection of rule files, memory and skills is suppressed so no instruction file outside the workspace can steer it. Its update check must be off. Approval-bypass switches are forbidden in every spelling, including environment variables. Outbound messaging, credential and service commands are never used. |
| Kilo Code CLI | JSON event stream; ACP optional | Run with an absolute working directory. Cloud, remote, share, plugin and import surfaces are disabled, and every way a session can be shared is closed. A restrictive permission map is **injected and then proven by reading back the resolved configuration**; it is default-deny with an explicit allow-list, not a list of specific denies, because several privileged tools are governed by no named permission key at all and only the wildcard rule closes them. Codebase indexing is switched off because it uploads code embeddings to a remote vector store — a second, independent egress path that disabling session sharing does not close. |
| OpenCode CLI | JSON event stream; ACP optional on the admitted 1.x line | The 1.x and 2.x lines differ in flags, environment variables and how configuration is injected, so the adapter is **version-aware and keys every launch, resume and determinism vector to the probed line**. The 1.x line is admitted; the **2.x line is unverified**, and the adapter must fail closed on a version it cannot classify. On 2.x it must run in standalone mode, because the default is a persistent background service shared across invocations, and its configuration discovery is not confined to the workspace — both are the recorded scope hazards set out below. The autonomous-approval flag is never used for mediated work. Network-serving, import, credential-export and plugin-install subcommands are never used. |

Rules common to all three:

- A process exit code is never the only sign of completion. Completion is derived from the observed event stream, and a clean exit with empty output counts as a failure.
- Credentials stay with the CLI and never enter a message payload.
- Every command an agent wants to run is mediated by the execution kernel.
- Where an agent exposes a permission map, the adapter injects **default-deny with an explicit allow-list** — the base rule denies everything, and only a small fixed set of read, search and file-editing tools is re-allowed — and then **proves the effective map by reading it back** rather than trusting what it injected. The agent capability surface is therefore: default deny → explicit minimal allow-list → controller-mediated execution → effective permissions verified → privileged and uncontrolled capabilities unavailable. Only explicitly authorized workspace capabilities are permitted: reading files, enumerating or searching authorized workspace content, and creating or modifying files within the authorized task scope. Privileged capabilities — autonomous sub-agent delegation, scheduling, external communication, credential operations, plugin installation, remote or cloud operations, and unrelated system control — are not available through the mediated agent surface unless an explicitly governed product capability later authorizes them. The allow-list is a deny-by-default rule rather than a list of specific denials, because several privileged tools are governed by no named permission key at all and only the wildcard rule closes them. Those tools therefore fall to deny, so **sub-agent delegation and scheduling are unavailable by default** — a consequence of the allow-list, not a separately configured rule. This applies to the agents that have a permission map; Hermes has none documented, and its controls are the suppression and prohibition rules above.
- Each agent session is tracked through a lifecycle with compare-and-swap transitions, health reports and process supervision.
- A task that needs a capability an agent does not have is given to an agent that has it, blocked, or escalated to the user; it is never allowed to pretend the capability exists. If a capability is lost while work is running, the actions that depended on it are invalidated and the work enters recovery.

**Two verification traps worth stating explicitly**, because both fail in the unsafe direction:

- **A permission check must be answered by a server the adapter started itself**, with a credential the caller chose — never by the CLI's own configuration-inspection commands. On the 2.x line those commands are answered by the persistent background service and ignore the invoking process's environment, so a gate that trusts them can certify a permission map the agent will never actually apply.
- **Configuration discovery is not confined to the authorized workspace.** On the 2.x line, discovery walks up from the agent's working directory, so a configuration file in a parent of the authorized workspace — or outside it entirely — can still contribute configuration. The adapter must enforce the boundary itself rather than assume discovery stops at the workspace.

### The communication fabric

All coordination travels over one protocol, MCF-v2. **There is no direct agent-to-agent channel.** A message always goes agent, adapter, bus, controller service, bus, adapter, agent.

- **Envelope.** Each message carries its protocol and schema version, message and event ids, project and session, sender and recipients, channel, message type, phase, correlation id, sequence number, project epoch, priority, timestamp, flags for ack, response and blocking, the payload and security data. Material-action messages also carry an operation id.
- **Vocabulary.** Message types and named events each have a declared, versioned definition in a machine-readable registry, with a declared emitter and owner.
- **Priority lanes, highest first:** emergency control, synchronization, task control, failure recovery, council, progress and heartbeat, bulk.
- **Delivery is at-least-once, made safe by idempotency.** Material actions are keyed by project id plus operation id, so a duplicate can never repeat a side effect.
- **An ACK means receipt and persistence only.** It never means the work succeeded. Success or failure is reported separately.
- **The bus persists before it acknowledges.** The sender writes a message and its outbox entry in one transaction; the receiver validates the envelope and stores it in an inbox before acknowledging.
- **Reliability behaviour.** Lanes are served in order. Failed deliveries retry with backoff, then expire or move to a dead-letter store. Ordering gaps are detected and reported, never silently repaired. A bounded queue answers "not yet" rather than failing. Replay is explicit, controller-invoked, produces a new message, and refuses material actions unless they are freshly authorized.
- **Bad traffic is refused explicitly, never absorbed.** Malformed, unauthorized, oversized and cross-project messages are rejected with a recorded reason.
- **History is immutable.** Events are append-only and chained with SHA-256 per project, so corruption, deletion or reordering is detectable. The chain is not keyed, so it does not resist a deliberate full recompute — a limitation the design records rather than hides.

## 6. The council

The council lets the three agents deliberate as a virtual council **without becoming one shared mind**. A controller-side council service owns the process. Agents contribute through ordinary messages on a dedicated council channel, and they never touch the council's records directly.

### How a deliberation runs

Independent analysis, proposals, cross-critique, rebuttal, revision, disagreement resolution, a targeted user interview, convergence on product and task scope, a debate on solutions, tools and architecture, a review of acceptance criteria and the validation plan, and finally a decision and lock. **Initial analysis happens before any agent sees another's proposal**, to reduce anchoring.

### What an agent can say

**Message vocabulary.** Council messages express ideas, proposals, questions, critiques, counterarguments, rebuttals, revisions, stances (agree, disagree, block, accept, reject, abstain), decision and lock candidates, and synthesis.

Each contribution becomes an **immutable position** with its author, the message it came from, its type, a concise rationale (never private chain of thought), the positions it responds to, and a list of claims. Every claim carries evidence references, and a claim with none must be labelled an assumption. A revision points to the position it supersedes.

- Ideas and proposals open positions. A critique must cite targets that the controller assigned.
- Agree, disagree, block, accept, reject and abstain are reasoned stances, not decisions.
- A question is only a candidate; it never prompts the user by itself.
- **An agent's decision or lock is only a candidate.** Only the controller, acting for the user, persists an authoritative decision or a hard lock.
- A synthesis may be submitted only by one participating agent acting as the round's temporary chair. It must cite every position it merges, it is a proposed merge rather than a decision, and it does not by itself settle a disagreement; a merge that consists only of the chair's own position is not a synthesis. A non-chair agent reviews every synthesis, and a coverage check confirms it cites every surviving position, so the chair can never certify its own summary. The chair holds no vote, tie-break or override.
- A transport acknowledgement is never agreement.
- Before critique, positions are frozen and every proposal is assigned at least one reviewer; self-review does not count.

### Rounds and how they end

A round moves through open, collecting responses, critique, rebuttal, revision, disagreement review, closing and sealed, with a waiting-for-user state and explicit timeout and non-participation handling. **Silence is never agreement.**

A round ends for exactly one reason: **converged** (a full round produced no new or changed position), **synthesized**, **cap reached** (five rounds by default), **escalated**, or **sealed with an open question**, which leaves the project waiting for the user. Convergence is a fixpoint test over positions, not a count of agreeing agents, so a majority vote is never the sole authority on architecture. Reaching the cap is not by itself evidence of disagreement, so it is recorded distinctly from escalation. When a round escalates, the user receives a structured packet: the competing positions with their strongest arguments, a conflict matrix naming the requirement IDs actually in dispute and each side's stance, and an advisory recommendation. The conflict matrix exists to expose the structure of the disagreement between requirements, not to tally votes. The recommendation is never persisted as a decision, and the packet's timeout behaviour is fixed to pause, so no answer, timeout or silence can be converted into assent. A timeout always pauses; it never counts as assent.

Material disagreement is kept as first-class state rather than smoothed away so a round can close. It can be resolved by evidence, revised away by the council, escalated to the user, or left standing to block the architecture lock.

### Keeping decisions high quality

The controller computes all of this from facts. Agents cannot set any of it, and none of it changes the message set or the round's states.

- **Deliberation happens around an explicit decision point.** Council work is not open-ended discussion: it turns on a material decision point, and every such point gets exactly one recorded mode. The record carries the mode, the inputs it was chosen from, the reason list, the version of the selector and the source of any override.

- **Deliberation depth is proportional to risk, and chosen, not negotiated.** The mode is picked deterministically from the decision class, the blast radius, prior validation failures and disputes, and any explicit user override. **Solo**: one agent plus validation, recorded with no round at all. **Review**: one independent reviewer. **Full**: the whole deliberation. No model chooses the mode and no agent may select or downgrade one. Escalation is upward only and only between rounds, triggered by repeated validation failure or a reviewer's block, and there is never a silent downgrade. The thresholds are configuration, not fixed constants.

- **The decision class is explicit.** Every decision is classed as `HARD_LOCK`, `SOFT_DECISION`, `ASSUMPTION` or `OPEN`. A hard lock is not a strong preference: changing one requires explicit reopening and an impact analysis.
- **Evidence grades are computed, never claimed.** The controller grades every claim as an assumption, cited or verified. A claim is cited only when every reference it makes resolves to a real fact, document or evidence record, and verified only when the controller itself ran the check or spike behind it and stored the result as evidence. A grade supplied by an agent is not representable and is ignored; an agent's own confidence about its own evidence carries no weight. A claim is load-bearing unless it is explicitly marked as supporting, and a position is only as strong as its weakest load-bearing claim.

- **A round cannot converge on an assumption.** A material round cannot seal while a surviving position still rests on a load-bearing assumption. It continues while rounds remain, and otherwise ends escalated or at the round cap under the same rules. This is a precondition on sealing the round, not a change to the convergence test, and it adds no outcome value.
- **Real independence.** Agreement is counted per lineage group, not per agent, so two agents that share a codebase — Kilo Code and OpenCode — count as one corroboration. Fewer than two groups is recorded as uncorroborated.
- **Roles.** The controller rotates proposer, skeptic and verifier roles each round, and picks the skeptic from a different lineage group than the leading proposal's author when it can. A role frames the work and carries no authority; failing a duty is recorded as non-participation for that duty.
- **Synthesis review.** A non-chair agent critiques every synthesis, and a coverage check confirms it cites every surviving position, so the chair cannot certify its own summary.
- **Budgets.** Caps on rounds, wall-clock time, tokens and spikes. Exhausting one ends the round with an existing outcome and never silently accepts anything. Missing token data is recorded as unavailable, never invented.
- **Decisions are revisited explicitly, never edited.** After execution, each decision is recorded as held, amended, reversed or stayed unresolved, and the record is append-only: nothing is updated or deleted. It is derived only from controller facts — validation results, an explicit reopening command, and decision supersession — so a reversal is produced by that reopening and never by an agent's claim, and a held outcome requires a validation evidence reference. The record is informational: it never changes routing, thresholds or authority. When reopening materially changes project truth, the project returns to the applicable prior phase and the epoch advances.

- **Every decision stays connected to the work.** A decision links back to the round and the positions that produced it, and forward to the requirements, tasks and validation that depend on it, so a completed task can be followed back to the deliberation that authorized it.
- **Offline agents.** If an agent drops mid-round, its non-participation is recorded, the round pauses, and the user can see who is offline.

### Questions and the user

Discovery questions concern the brief and come before analysis. Interview questions come from the council after deliberation. The two never overlap, and free text typed outside them is recorded as a contribution, never as a bypass of the flow.

Agents' questions are normalized, clustered and de-duplicated, checked against the workspace (and public sources for research tasks), ranked by impact, and asked of the user **only if still unresolved**, in one batch.

An answer is saved as an immutable record and sent only to the affected agents, never broadcast to unrelated agents. If it changes material truth, the project epoch advances and a new immutable context snapshot goes out as a context update. A linked continuation round then opens. An answer is not automatically a requirement, a decision or a permission, and it does not bypass any mediated command. Redistribution is tracked as delivery and synchronization separately from transport acknowledgement, so an answer is not reported as applied until the affected agents have resumed from the current context. No answer, timeout or silence is ever converted into assent.

## 7. Contracts, correctness and governance

A structural promise runs through the whole design: **the machine-readable contract is the product.** Every message type, event, payload, state machine, error code and bridge operation has a declared, versioned definition, and the definitions are checked against each other and against the implementation rather than kept in prose beside it. The contract is versioned and machine-readable, covering messages, events, payloads, state machines, error codes and bridge operations. It is described by kind rather than by count, because the vocabulary grows as the system evolves.

**Verification is local, and deliberately so.** Everything that checks Mayasaba runs on the user's own Windows machine: there is no hosted continuous integration, and no cloud runner is used even when an equivalent hosted one exists. This follows from the same boundary that shapes the rest of the product — execution belongs on the user's PC, so moving verification to a hosted machine would violate the boundary rather than satisfy it. The rule is enforced rather than merely stated: a proposed change that reintroduces a hosted pipeline is rejected.

Correctness is layered rather than assumed. A contract check proves the definitions agree with each other and with the implementation; it does not compile or run anything, so it can pass while the build is broken. A separate verification pass covers format, compilation, build, the full test suite and the desktop tests, and stops at the first failure. Beyond those, the checks themselves are tested: known drift is reintroduced one case at a time, and the corresponding check must fail and name the specific disagreement it exists to catch.

**A validation result is bound to what produced it.** It is always about particular artifacts, a particular workspace, a particular environment and a particular version of the validator or test suite, so a run records the environment snapshot alongside the artifact hashes it was produced against.

**Change discipline.** Every material architectural change is classified as one of four kinds — additive, refinement, replacement or deprecation — and recorded with its previous behaviour, new behaviour, reason, compatibility impact, migration path and affected tests. Existing terms, ownership and sources of truth must not be silently redefined. Historical records are never erased to make the current state look tidier; a superseded decision is superseded *explicitly*, with traceability preserved. When two sources genuinely conflict, the conflict is recorded and traced rather than resolved quietly.

**Evidence-backed completion** is the point of all of it. Until that evidence exists, a completion claim is **untrusted** — not a pending fact and not a partial success. A feature is not complete because code was written, and not because an agent said so: completion requires the requested local artifact, the checks appropriate to the task, and the evidence that connects the two. Evidence carries immutable provenance and a content hash, so a bare file path can never be cited as evidence, and capturing it never widens the workspace boundary it was taken from.
