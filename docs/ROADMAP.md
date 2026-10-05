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

**Status: in progress.** Tranches 0 to 3 have landed and are validated: the decisions are recorded
(`025183b`), the shell holds a bus with a production clock and identity source and nothing dispatches
(`cd27580`), and the two read-only diagnostics are wired (`get_communication_health`, `get_event_cursor`). The
gate reports 7 of 59 declared operations implemented. Tranche 4 - the frontend bridge - is
not built, so no part of this milestone may be read as complete. `replay_event` stays declared with no handler, and `cancellable` is declared on 24 operations and honoured by none (DEC-075).

Deliberately out of scope: agent adapters and any real CLI process (M3), a real transport, a background
dispatch loop, `request_event_resync`, `PolicyService`, `ContextService`, an enqueue command, council runtime,
and any schema or table change.
## M3 — Agent gateway

M3 is blocked until M0.5, M1 and the native adapter probe contract are complete.
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

## Release gate

No release is considered production-ready until protocol, adapter, recovery, workspace, execution, validation and certification tests pass.
