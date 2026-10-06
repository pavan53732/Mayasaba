# Mayasaba Roadmap

## M0 — Repository and architecture foundation
- canonical repository
- documentation index
- master specification
- requirements
- subsystem architecture
- design governance
- decision register
- design history

## M0.5 — Workspace and contract integration seam
- repository workspace skeleton
- root/app/Cargo/npm/Tauri manifest contracts
- 12-crate dependency matrix
- canonical MCF-v2 registry wired to protocol/bus/core
- Tauri bridge schema and generated Rust/TypeScript type path
- empty-schema bridge round-trip test
- Windows local process smoke-test harness
- adapter probe-result schema
- no real agent required to pass the seam test

## M1 — MCF-v2 machine-readable contract
- JSON Schemas
- manifest
- message/event registries
- transition registry
- fixtures
- compatibility checker
- schema validation tests

## M2 — Durable local communication bus
- SQLite event log
- transactional outbox
- receiver inbox
- routing
- ACK/NACK
- idempotency
- ordering/gap handling
- retry/backoff
- priority lanes
- backpressure
- dead letters

**Status: implemented.** `crates/bus` and its `crates/storage` support implement the whole list above: the
per-project SHA-256 event chain over RFC 8785 JCS that the event log rests on (DEC-034), the envelope-gated
transactional outbox with live-claim idempotency (DEC-058, DEC-065), the pure dispatch decision with a policy file,
a transport trait, bounded retry and backoff and dead letters (DEC-058), the durable inbox with persist-before-ACK
and receipt-only ACK (DEC-059), the declared priority lanes in the contract's order (DEC-060), requeue (DEC-061),
`RETRYING -> EXPIRED` (DEC-062), ordering gap detection (DEC-063), backpressure as measurement (DEC-064) and
explicit controller-invoked replay (DEC-065). Two items are deliberately not done and are recorded rather than
implied: backpressure does not **refuse** an enqueue, because the error registry has no code for transient capacity
(DEC-064), and the bus ships no production clock, transport or identity source, because it performs no I/O
(DEC-058, DEC-065).
- explicit replay

## M2.5 — Shell integration for the durable bus

M2 built the bus and validated it; nothing in the running application reaches it. This milestone wires it into
the Tauri shell **without faking delivery**. There is no transport until M3, so no dispatch loop runs and no
message is handed to something that cannot carry it.

Decided in DEC-069 to DEC-074: a reported ordering gap is surfaced as a diagnostic rather than resynchronised;
`CAPACITY_EXCEEDED` is retried by the shell and rendered as busy rather than failed; replay refuses
material-action messages until `PolicyService` exists; the Bus holds its own `Storage` connection and new
handlers are `async` with work on `spawn_blocking`; no background dispatch runs; the production `Clock` and
`IdSource` live outside `crates/bus`.

**Status: in progress.** Tranches 0 to 4 have landed and are validated: the decisions are recorded
(`025183b`), the shell holds a bus with a production clock and identity source and nothing dispatches
(`cd27580`), and the two read-only diagnostics are wired (`get_communication_health`, `get_event_cursor`). The
gate reports 7 of 59 declared operations implemented. Tranche 4 added the typed frontend bridge functions and their tests, which no component renders yet: the milestone is complete for its declared tranches, and nothing in the Control Room displays these answers. `replay_event` stays declared with no handler, and `cancellable` is declared on 24 operations and honoured by none (DEC-075).

Deliberately out of scope: agent adapters and any real CLI process (M3), a real transport, a background
dispatch loop, `request_event_resync`, `PolicyService`, `ContextService`, an enqueue command, council runtime,
and any schema or table change.
## M3 — Agent gateway

**Status: in progress.** The contract-driven adapter kernel, closed three-agent identity set, launch-proof boundary,
Windows executable discovery, version probing, persisted installation/capability facts, agent-session CAS lifecycle,
live supervised process launch and incremental native-event normalization are implemented. Remaining M3 work is durable MCF transport delivery. Contract-derived capability admission, native session-id reconciliation and supervised-process health monitoring are implemented.
- Hermes Agent CLI adapter
- Kilo Code CLI adapter
- OpenCode CLI adapter
- runtime discovery
- handshake
- capability negotiation
- health checks
- process supervision
- structured transport integration

## M4 — Context and synchronization
- project epochs
- immutable ContextPacks
- state digests
- deltas
- stale-context enforcement
- affected-context dependency mapping
- recovery rehydration

## M4.5 — Council collaboration proof (simulated)
Before broad task execution or full Control Room implementation, prove Mayasaba's central collaboration loop with three deterministic simulated participants — one per supported agent under DEC-029 — and the real CouncilService/MCF/context path:
- independent proposals on one shared project snapshot;
- controller-assigned critique targets, references to prior positions, and rebuttal/revision;
- a material unresolved question deduplicated and shown in a minimal Control Room question surface;
- a user answer persisted through `answer_user_question`, followed by a scoped `CONTEXT_UPDATE`, a new/current ContextPack and agent revision;
- a recorded council outcome/decision candidate without allowing an agent to commit an authoritative lock;
- a persisted mode selection for each simulated material decision point, including a `SOLO` record with no round;
- controller-computed claim grades and lineage-group corroboration for the simulated positions.

No live AI CLI is required for this proof. The slice passes only when the question/answer is durable, targeted agents receive the new context, stale context cannot authorize work, duplicate delivery is safe, and non-participation is explicit. M6 completes the production Council Engine after this behavior is demonstrated.

## M5 — Task/workspace execution for code and local artifacts

**Status: reliability/runtime foundation implemented; full milestone remains open.** Durable task leases, fencing,
attempt identity, deterministic ready-task selection, command execution records, process observations, bounded local
process supervision, restart reconciliation, durable artifact/evidence/validation persistence, file hashing and
execution artifact binding are implemented. Git worktree command isolation/observation specifications are also
implemented. Actual controller-mediated worktree materialization/integration, full workspace mutation policy,
runtime build/test/evidence orchestration and end-to-end project execution remain open.
- request/acceptance-criteria-to-task DAG
- task leases
- lease renewal/expiry
- proof-carrying handoffs
- user-selected workspace and task-scoped allowed paths
- Git worktrees for software repositories
- non-Git/file-focused isolation
- checkpoints and outside-scope approval
- local command execution and project-local installs under policy
- preview/recovery for bulk or destructive data edits
- task-appropriate evidence and validation gates
- process lifecycle

## M6 — Council
- independent analysis
- proposals
- cross-critique
- rebuttal/revision
- disagreement resolution
- user question clustering/deduplication
- decision locks
- council barriers
- participation/timeout semantics
- deterministic council modes and controller-owned mode-selection records
- controller-computed claim grades, lineage-group corroboration and round roles
- independent non-chair synthesis review, council budget caps and pause-on-offline failure handling
- append-only decision outcome tracking, informational only

## M7 — Software build/run/test/E2E
- toolchain discovery
- repository-based command discovery
- build execution
- app launch
- runtime verification
- deterministic tests
- browser/UI validation where applicable

## M8 — Review/repair
- cross-agent review
- failure fingerprints
- diagnosis
- repair tasks
- bounded retries
- checkpoints/rollback
- regression testing
- high-severity issue gates

## M9 — Control Room
- project management
- chat/timeline
- council view
- requirements/architecture/decisions
- tasks
- agent sessions
- files/worktrees
- local document/report/data artifact review and preview
- build/run/test where applicable
- repair
- evidence
- communication observability
- pause/resume/stop

## M10 — Evidence-backed end-to-end delivery
- software path: idea → working project, with synchronization, build/test/repair and certification
- local-artifact path: user request → scoped document/report/data/code changes → task-appropriate validation and evidence
- read-only public-source research and local citation/report delivery when requested
- certification against only the applicable acceptance criteria and gates

## M11 — Packaging
- final evidence bundle
- final validation
- MSI generation
- installation validation

## Cross-cutting — Attachment references (DEC-049, DEC-106, DEC-107)

Attachments belong to no single milestone, and that is why the requirement was orphaned: `DATA-MODEL.md`
documented `ProjectContextAttachment` and DEC-106 settled its storage model, while no milestone listed the work,
so nothing tracked it. It is recorded here as a cross-cutting capability instead of being assigned to a milestone
that would own only part of it.

An attachment is a durable local reference with captured provenance: the user selected this path, at this time,
inside this authorized scope, and nothing more. It is supporting context and evidence, never project truth
(DEC-049). Attaching is one of three operations that are deliberately not collapsed (DEC-106) — attach records
the reference, capture computes a content hash and produces evidence (DEC-102), consume is an owning service
accepting a material change. Attachment is optional on both surfaces and is never a prerequisite for creating a
project or for submitting a normal user message.

- canonical `project_context_attachments` table with a CHECK-closed kind, provenance and lifecycle vocabulary
- `AttachmentService` owns the entity, its association and its lifecycle; `WorkspaceService` owns locality and
  authorized-scope validation; `EvidenceService` owns capture, hashing and evidence provenance (DEC-107)
- path validation: locality, existence, kind and component-wise containment, with the scope read from the
  project's stored workspace root rather than accepted from the caller (DEC-048)
- read-time resolvability, reusing the `Admission` shape of per-check status plus one verdict
- `attach_project_context_attachment`, `list_project_context_attachments` and
  `resolve_project_context_attachment` on the bridge, with shape-tested wire types
- the Initial Intake Composer and the Ongoing Chat Composer both attach, with one shared presentation state
- **capture** (hash plus Artifact/Evidence) and **consume** (explicit acceptance) — declared by DEC-106,
  deliberately deferred, and not implied by attaching

**Status: the attach path and both surfaces are implemented; capture and consume are not.** The table, the
service, the three bridge operations and the shared tray presentation are in place and tested on both the Rust
and the TypeScript sides, and the intake and chat composers both attach. Three things are deliberately not done
and are recorded rather than implied: nothing sets `content_hash` or `context_evidence_id`, because `capture`
and `consume` have no operation and attaching must not perform them; a directory is attached as a scope and
never enumerated, so nothing durable describes its contents; and whether two attachments may reference one
`source_path` in a single project remains deliberately open (DEC-106).

## Cross-cutting — UserContribution (DEC-030)

Recorded here rather than under a milestone, for the same reason as the attachment block above: DEC-030 requires
"a new Tauri command for classified free-text input", no milestone listed it, and the requirement sat recorded
in `App.tsx` as a gap the composer stated instead of inventing.

A `UserContribution` durably records a free-text message submitted after project creation, with the advisory
classification it was routed under and the outcome the owning service produced. It is a record of what the user
contributed and never an authority for project truth: only the owning authoritative service determines
materiality, and only a material change increments `project_epoch` (DEC-030).

- `ProjectService` in `crates/core` owns the record and the command, because DEC-030 makes a material
  contribution increment `project_epoch` and `ProjectService` already owns `projects`, `project_briefs` and the
  epoch. The advisory classification stays with the intake router as ConfigurationService policy
- `record_user_contribution` on the bridge, with shape-tested wire types and the owner declared in
  `workspace.manifest.json` and `schemas/service-contracts-v1/registry.json`
- the three vocabularies closed by CHECK constraints in `schemas/sqlite-v1/schema.sql`: `classification` is
  `MATERIAL`/`CONTEXT`/`COMMENTARY`, `classification_source` is `INTAKE_ROUTER`, and `result_type` is
  `EPOCH_ADVANCED`/`CONTEXT_SNAPSHOT`/`NO_CHANGE`/`PENDING`
- the Ongoing Chat Composer records the message and renders the row the service stored, not the draft it sent
- **routing** — carrying the text to the owning service that would decide materiality and produce a real
  outcome, replacing `PENDING` — is declared by DEC-030 and deliberately not done

**Status: recording is implemented; routing is not.** The command, the service operation, the table and the
composer are in place and tested, and the append-only rule holds: there is no update path and no delete path, so
a later ruling is a later row. What is deliberately not done is the routing itself, so every recorded row carries
`result_type = PENDING` with `epoch_after = epoch_before`. That is the honest record — the user contributed this
text and it was labelled for routing — and the composer names the unchanged epoch pair on the surface so the
advisory label cannot be read as a change to project truth. Nothing yet reads a contribution back for display in
a timeline, because no operation lists them.

## Cross-cutting — Trace-link operation ownership (open)

`create_trace_link` is declared in two files and owned by no service in
`schemas/service-contracts-v1/registry.json`. The two declarations now agree, and the ownership cross-check in
`tools/contracts/verify.mjs` keeps them agreeing, but no canonical document names the service that should own
trace links. No traceability service exists; `RequirementService` is the manifest's value and `DiagnosticsService`
was `payloads.json`'s. Until a decision names the owner, the agreed value is a consequence of the manifest being
authoritative rather than of a decision, and the generated UI owner map is only as right as that value.

## Release gate

No release is considered production-ready until protocol, adapter, recovery, workspace, execution, validation and certification tests pass.


## Cross-cutting reliability gate

The following reliability capabilities are acceptance requirements across M3-M10; their durable storage/contracts and controller recovery-planning foundations are now implemented, while live adapter/process enforcement lands with the corresponding milestones. They are not a replacement milestone and do not alter the locked agent set (DEC-029):
- durable TaskAttempt identity across retry/reassignment;
- lease-version fencing on every material write derived from a lease;
- scheduler resource admission for ports/process slots and other scarce local resources;
- WorkspaceRevision and EnvironmentSnapshot provenance;
- persisted safe points and recovery reconciliation;
- plan/context invalidation on material epoch changes;
- certification binding to exact artifact/workspace/environment/validator inputs;
- explicit UNKNOWN state and desired-vs-observed reconciliation;
- deterministic liveness/convergence, fairness and bounded repair/swarm budgets.
- Windows process containment is atomic: suspended spawn → Job Object assignment → resume; termination is controller-owned and does not invoke a shell utility.
- agent discovery/session/process identity remains durable across restart; task leasing is impossible before an agent session reaches READY.
- lease expiry is a closed recovery loop: the lease becomes EXPIRED and the owning Task becomes LEASE_EXPIRED atomically; TaskService
  stages RECOVERY_PENDING and returns the Task to READY only after no live lease, unresolved attempt or live/cleanup/unknown process
  execution remains. Terminal execution outcomes such as completed timeout/cancel/crash records are historical and do not block readiness.
