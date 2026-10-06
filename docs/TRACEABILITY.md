# Mayasaba Requirements-to-Evidence Traceability

## Purpose
Every material requirement must be traceable through design, implementation, validation, and certification.

## Canonical chain
USER INTENT -> REQUIREMENT -> ACCEPTANCE CRITERIA -> DECISION / HARD_LOCK -> ARCHITECTURE / CONTRACT -> TASK -> TASK ATTEMPT / TASK LEASE / AGENT -> CHANGESET / ARTIFACT -> TASK-APPROPRIATE CHECKS (EXECUTION / TEST / SOURCE REVIEW / INTEGRITY) -> EVIDENCE -> REVIEW -> VALIDATION -> CERTIFICATION

## Intent anchor
USER INTENT is the `ProjectBrief` version that was current when DISCOVERY closed. The existing `INTENT_REQUIREMENT` link type connects that brief version to the requirements derived from it. Because brief versions are immutable, the anchor for a lineage is stable: a later brief version starts a new derivation path rather than rewriting the links of an earlier one.

## Traceability references
Requirements may link to acceptance criteria, decisions, architecture artifacts, contracts, tasks, commits/changesets, executions, tests, evidence, reviews, and validation runs.

Admission records are traceable objects: a `WORKSPACE_ADMISSION` links a task lease to the workspace it was admitted into, and an `INTEGRATION_ADMISSION` links a changeset to the integration decision that let it through. Because the record carries its own evidence and verdict, "changed files without an owning task" and "integrations without an admission" are both detectable.

Decision-quality records — mode selections, claim grades, round roles, budget ledger entries and decision outcomes — attach to the existing canonical chain at council round → decision → validation. They add no new trace link type: the canonical link-type vocabulary is closed, and a new type would require its own decision.

## Coverage

NOT_ADDRESSED -> ADDRESSED -> IMPLEMENTED -> VALIDATED -> CERTIFIED

Implemented does not mean validated. Certification is the controller's; no table in this document may assert it.

Two further state words are part of this closed vocabulary, because the tables below already used them without
declaring them:

- `DECIDED` — the governing decision is recorded and nothing is implemented. It is a more specific `ADDRESSED`.
- `OBSERVED` — evidence exists, but it comes from another milestone's test rather than one written for this row.
  It is a weaker `VALIDATED` and never a substitute for it.

A state cell is a state word followed by an optional parenthetical qualifier, as in `VALIDATED (T2)`.

The vocabulary is closed and enforced, together with three properties that keep the rows from claiming more than
they can support: every cited evidence path must resolve, every cited decision must have a `DECISION-REGISTER.md`
entry, and a `VALIDATED` row must cite evidence. `tools/contracts/verify.mjs` fails on a violation, so a renamed
file or a withdrawn decision breaks the commit rather than leaving a row that reads as proven.

## Orphan detection
Detect requirements without tasks, tasks without requirements or explicit rationale, implementations without a valid task lease, changed files without an owning task, validations without scoped requirements, and certification claims without required evidence.

## Change impact
Changes to requirements, decisions, or contracts identify affected tasks, contexts, validations, and evidence and advance the project epoch according to the canonical context rules.

## Authority
Traceability indexes relationships between authoritative objects. It is not a second source of truth.

## M2 communication bus coverage

The M2 slice (`crates/bus`, with its `crates/storage` support) is **IMPLEMENTED and VALIDATED**, not CERTIFIED:
certification is the controller's, and no controller has certified it. Each requirement below reaches VALIDATED
through the tests named, and each decision is a record in `docs/DECISION-REGISTER.md`.

| Requirement | Decision | Evidence |
| --- | --- | --- |
| Durable event log with a per-project hash chain | DEC-034 | `crates/storage/tests/event_chain.rs`, `canonical.rs` |
| Transactional outbox, envelope-gated, idempotent | DEC-058, DEC-065 | `crates/bus/tests/enqueue.rs` |
| Pure dispatch decision, policy file, bounded retry, backoff, dead letters | DEC-058 | `crates/bus/tests/dispatch.rs` |
| Durable inbox, persist-before-ACK, receipt-only ACK, NACK reason, duplicate delivery | DEC-059 | `crates/bus/tests/inbox.rs` |
| Declared priority lanes in the contract's order | DEC-060 | `crates/bus/tests/dispatch.rs`, gate check |
| Requeue, and event identity across a repeated transition | DEC-061 | `crates/bus/tests/inbox.rs` |
| `RETRYING -> EXPIRED`, closing the last declared exit | DEC-062 | `crates/bus/tests/inbox.rs` |
| Ordering gap detection | DEC-063 | `crates/bus/tests/inbox.rs` |
| Backpressure as measurement | DEC-064 | `crates/bus/tests/dispatch.rs` |
| Explicit controller-invoked replay | DEC-065 | `crates/bus/tests/replay.rs` |
| The delivery code held to the declared machine | DEC-066 | `npm run verify:contracts`, mutation `dec066-a` |

**Not addressed, and recorded as such rather than implied.** Backpressure does not refuse an enqueue, because the
error registry has no code for transient capacity (DEC-064); no production clock, transport or identity source is
shipped, because the bus performs no I/O (DEC-058, DEC-065); four of the machine's thirteen edges are not read by
the gate, because they are inline SQL where the from-state is implied by a `WHERE` clause (DEC-066). The authorization decision is persisted as the canonical `AUTHORIZATION_VALIDATED` event in the immutable event chain (owned by `PolicyService`) rather than as an outbox table column, resolving that gap.


## Shell integration coverage (M2.5)

Requirements for wiring the bus into the shell, mapped to the decisions that govern them and to the evidence
that exists once each tranche lands. **Tranches 1 to 4 are validated** (`cd27580`, the T2 commit and the T3 commit); every row below is backed by an observed test.

| Requirement | Decision | Evidence | State |
| --- | --- | --- | --- |
| A reported ordering gap is visible to the operator | DEC-069 | `open_sequence_gaps` derives holes from `messages`; a test asserts `SEQUENCE_GAP` with session, channel, expected and found | VALIDATED (T2) |
| A gap is not repaired by discarding the arrival | DEC-069 | The derivation only reads `messages`; the same test asserts `total_changes` is unchanged across both diagnostics, so no arrival was removed | VALIDATED (T2) |
| Resynchronisation is not claimed | DEC-069 | `request_event_resync` remains declared with no handler; the gate reports 6 of 59 implemented and it is not one of them | VALIDATED (T2) |
| Capacity refusal is transient to the caller | DEC-070 | `retry_on_capacity` stops at its bound and returns the last refusal; a test asserts exactly `max_attempts` calls | VALIDATED (T1) |
| Capacity is not rendered as failure | DEC-070 | UI renders queued/busy; the code is never mapped to a non-backoff registry code | DECIDED |
| Replay cannot bypass authorization | DEC-071 | Material-action replay is refused with `AUTHORIZATION_NOT_IMPLEMENTED`; a test asserts nothing is enqueued and the source keeps its state | VALIDATED (T3) |
| A replayed message is not presented as context-fresh | DEC-071 | Response carries the original snapshot and digest with `context_refreshed: false`, asserted over a message whose envelope carries both | VALIDATED (T3) |
| SQLite work does not block the IPC thread | DEC-072 | Both new handlers are `async` and reach the bus only through `on_bus`, which uses `spawn_blocking`; the vendored macro source shows a sync command runs inline on the delivery thread | VALIDATED (T2) |
| Two connections to one file are safe | DEC-072 | The M2 concurrent-writer test observes two writers landing without loss under the rollback journal and a 5000 ms busy timeout | OBSERVED (M2) |
| No message is dead-lettered by a dispatcher that cannot deliver | DEC-073 | A test constructs the shell state and asserts nothing is dead-lettered, and a structural guard fails if a dispatch call site is added to `bus_shell.rs` or `main.rs` | VALIDATED (T1) |
| Clock and identity are real outside the bus | DEC-074 | `CoreClock` and `CoreIdSource` in `crates/core`; tests assert the fixed-width UTC shape, the v4 version/variant nibbles, and uniqueness over 20 000 draws and across 8 threads. `crates/bus` gains no I/O | VALIDATED (T1) |

Not addressed by this milestone: a real transport, adapters, `PolicyService`, `ContextService`,
`request_event_resync`, and an enqueue command. Each is out of scope by decision rather than by omission, and
each is named in DEC-069 to DEC-074.

## Attachment coverage (DEC-049, DEC-106, DEC-107)

An attachment is context and evidence, never project truth (DEC-049). It is a durable local reference with
provenance (DEC-106), and the reference is deliberately not the same act as reading the file: attach records a
reference, capture reads the contents, and consume accepts a material change. Only the first has an operation.

`CONTROL-ROOM-DESIGN.md` requires that an attachment never be presented as uploaded, indexed, analysed or
accepted until the controller has returned that state. The rows below are what exists to support that claim, and
the rows at the end are what does not exist, recorded rather than implied.

| Requirement | Decision | Evidence | State |
| --- | --- | --- | --- |
| A selected file or folder becomes a durable reference carrying its provenance, and nothing else | DEC-106 | `attach_records_a_reference_with_provenance_and_no_content_identity` in `crates/core/tests/attachment_slice.rs` | VALIDATED |
| Attaching reads no content: it does not hash, copy, index or open the source | DEC-106 | `attach_never_reads_the_file_contents` in `crates/core/tests/attachment_slice.rs`; the storage writer's request type has no field for a hash or an evidence link, so the operation cannot record one | VALIDATED |
| The authorized scope is read from the project, never from the caller | DEC-048 | `the_scope_is_read_from_the_project_and_never_from_the_caller` in `crates/core/tests/attachment_slice.rs`; `apps/desktop/src/intake/bridge.test.ts` asserts no scope argument crosses the wire | VALIDATED |
| A path outside the authorized scope is refused although it exists | DEC-048 | `a_path_outside_the_scope_is_refused_even_though_it_exists` and `a_sibling_whose_name_shares_a_prefix_is_not_inside_the_scope` in `crates/workspace/tests/attachment_validation.rs` | VALIDATED |
| Locality, existence and kind are validated before anything is persisted | DEC-106 | `a_network_location_is_refused_as_out_of_scope_rather_than_missing`, `a_nonexistent_source_is_refused_and_nothing_is_created` and `an_empty_candidate_is_refused_before_anything_is_inspected` in `crates/workspace/tests/attachment_validation.rs` | VALIDATED |
| A canonical local path is not mistaken for a network location | DEC-106 | `a_canonicalized_local_path_is_not_mistaken_for_a_network_path` and `a_verbatim_unc_path_is_still_a_network_path` in `crates/workspace/tests/attachment_validation.rs` | VALIDATED |
| A directory is attached as a scope and never enumerated into a snapshot | DEC-106 | `a_directory_is_attached_as_a_scope_and_not_as_a_snapshot` in `crates/core/tests/attachment_slice.rs` | VALIDATED |
| Resolvability is checked when the reference is read, never stored as a status | DEC-106 | `a_live_reference_resolves_with_every_check_passing` and `a_modified_source_still_resolves_because_content_identity_is_not_implied` in `crates/core/tests/attachment_slice.rs`; `crates/core/src/attachment_service.rs` derives the four checks on each read | VALIDATED |
| A deleted source leaves the row in place and is reported as an outcome, not as an error | DEC-106 | `a_deleted_source_leaves_the_row_and_is_reported_as_an_outcome` in `crates/core/tests/attachment_slice.rs` | VALIDATED |
| A moved source is never rewritten into the stored row | DEC-106 | `a_moved_source_is_not_rewritten_into_the_row` in `crates/core/tests/attachment_slice.rs` | VALIDATED |
| Re-attaching one path is a new identity rather than an update to the old one | DEC-106 | `re_attaching_the_same_path_is_a_new_identity_rather_than_an_update` in `crates/core/tests/attachment_slice.rs` | VALIDATED |
| The two composer surfaces are distinguishable in the stored provenance | DEC-106 | `provenance_distinguishes_the_two_composer_surfaces` in `crates/core/tests/attachment_slice.rs` | VALIDATED |
| An unknown project is refused before the path is inspected, and is not reported as a missing attachment | DEC-055 | `attaching_to_an_unknown_project_is_refused_before_validation` in `crates/core/tests/attachment_slice.rs`; the shell maps the two conditions to `PROJECT_MISMATCH` and `ATTACHMENT_NOT_FOUND` | VALIDATED |
| Attachment rejections carry distinct registered codes with human messages | DEC-055 | `attachment_rejections_carry_distinct_machine_readable_codes` and `every_attachment_rejection_has_a_human_message` in `crates/workspace/tests/attachment_validation.rs` | VALIDATED |
| The entity and its lifecycle belong to `AttachmentService`, path and scope validation to `WorkspaceService`, and capture to `EvidenceService` | DEC-107 | `workspace.manifest.json` and `schemas/service-contracts-v1/registry.json`; the gate fails when the service registry and the manifest disagree | VALIDATED |
| The canonical table is declared, indexed and referenced by the data model | DEC-106 | `schemas/sqlite-v1/schema.sql`, `docs/DATA-MODEL.md`; the gate fails when a core entity has no table or a table is undocumented | VALIDATED |
| The three operations are declared on the bridge with shape-checked wire types | DEC-107 | `schemas/tauri-bridge-v1/payload-types.json`, `apps/desktop/src-tauri/src/main.rs`; the shell's conformance tests serialize each real struct against the declaration | VALIDATED |
| Both composers share one attachment presentation state | DEC-106 | `apps/desktop/src/attachments/state.ts` and `apps/desktop/src/chat/state.ts`; `apps/desktop/src/chat/state.test.ts` asserts the chat surface delegates tray actions rather than re-implementing them | VALIDATED |
| No attachment is presented as uploaded, indexed, analysed or accepted before the controller returned that state | DEC-106 | `apps/desktop/src/attachments/state.test.ts`; an entry is durable only when a resolution came back from Rust, and the projection carries the lifecycle that answer reported | VALIDATED |
| An attachment is never a prerequisite for submitting a normal message | DEC-106 | `apps/desktop/src/chat/state.test.ts`; `canSendMessage` takes the message text and no tray, so no attachment can change the answer | VALIDATED |
| Both surfaces render the tray, pick through the bridge and attach once a project exists | DEC-106 | `apps/desktop/src/App.tsx` | IMPLEMENTED |
| Attaching is optional at intake and never gates project creation | DEC-106 | `apps/desktop/src/App.tsx`; `canSubmit` reads the brief and the workspace and not the tray | IMPLEMENTED |
| Capture: hash the contents and produce an artifact and evidence record | DEC-102 | — | NOT_ADDRESSED |
| Consume: an owning service accepts a material change caused by the contents | DEC-106 | — | NOT_ADDRESSED |
| An attachment cannot be cited as evidence before it is captured | DEC-102 | — | NOT_ADDRESSED |
| Detaching an association without deleting the row | DEC-106 | — | NOT_ADDRESSED |
| A chat message is recorded as a classified user contribution | DEC-030 | `crates/core/tests/user_contribution.rs` records one end to end and reads the row back through `crates/storage/src/lib.rs`; `apps/desktop/src/chat/state.test.ts` covers the composer's `recorded` state | VALIDATED |
| Whether two attachments may reference one `source_path` in one project | DEC-106 | — | DECIDED |
| `ProjectService` owns the record and the command that writes it | DEC-030 | `workspace.manifest.json` and `schemas/service-contracts-v1/registry.json` declare the owner, `schemas/tauri-bridge-v1/payloads.json` declares the operation, and `tools/contracts/verify.mjs` fails when the three disagree | VALIDATED |
| The advisory classification is stored but never acted on: recording a `MATERIAL` label does not advance the epoch | DEC-030 | `crates/core/tests/user_contribution.rs`; `a_material_label_is_advisory_and_does_not_advance_the_epoch` records a `MATERIAL` label and asserts the project's `current_epoch` is unchanged | VALIDATED |
| The `classification`, `classification_source` and `result_type` vocabularies are closed | DEC-030 | `schemas/sqlite-v1/schema.sql` CHECK constraints; `crates/core/src/project_service.rs` refuses an undeclared label before anything is written | VALIDATED |
| A refusal writes no row, and the stored body is the trimmed text that was validated | DEC-030 | `crates/core/tests/user_contribution.rs`; the unknown-project, undeclared-classification and whitespace-only cases each assert the project holds no contribution | VALIDATED |
| Routing a contribution to the owning service that would decide materiality | DEC-030 | — | NOT_ADDRESSED |

The `IMPLEMENTED` rows are the shell wiring, and they are deliberately not `VALIDATED`: `apps/desktop/src/App.tsx`
has no test, because the desktop suite runs without a DOM. The two `NOT_ADDRESSED` capture rows are the reason
`content_hash` and `context_evidence_id` are nullable and empty in every stored row. The `source_path` row is
open by decision rather than by omission: DEC-106 leaves the question unanswered, and nothing in the schema
enforces an answer in either direction.

The `UserContribution` rows are `VALIDATED` for the Rust path and for the composer's reducer. Recording is wired
through `apps/desktop/src/App.tsx` the same way the tray is, which is why that surface is `IMPLEMENTED` and not
`VALIDATED` for the same reason as the rows above. What is **not** addressed is routing: `record_user_contribution`
stores `result_type = PENDING` with an unchanged epoch pair, because no operation carries the text to the owning
service that would decide materiality. The record therefore claims no epoch effect, which is why recording a
`MATERIAL` label is asserted to leave `current_epoch` alone.

## Persistence implementation

Traceability is persisted by the SQLite storage owner.

Canonical tables:
- `trace_links` — typed source → target relationship.
- `trace_link_versions` — immutable supersession/history.
- `trace_coverage` — materialized coverage facts derived from authoritative links and validation/certification state.

A trace link is validated by `schemas/trace-v1/trace-link.schema.json`; its `link_type` is the closed vocabulary of the canonical chain above.

The `create_trace_link` operation's owner is declared twice: in `workspace.manifest.json` and in
`schemas/tauri-bridge-v1/payloads.json`. `payloads.schema.json` states that the gate resolves an operation's
`owner` against the manifest, and `tools/codegen/generate-bridge.mjs` reads it from `payloads.json` to emit the
map the frontend routes calls by. Nothing compared the two until the ownership cross-check was added to
`tools/contracts/verify.mjs`, and they had drifted: `payloads.json` named `DiagnosticsService` while the manifest
named `RequirementService`, so the generated UI map named a different authority than the contract did. The copy
was corrected to the manifest, because the contract declares the manifest authoritative. **No canonical document
names the service that owns trace links**, and `schemas/service-contracts-v1/registry.json` lists
`create_trace_link` under no service at all, so the agreed value follows from which file is authoritative rather
than from a decision. It is recorded as an open ownership question, not presented as settled.

Canonical link types:
`INTENT_REQUIREMENT`, `REQUIREMENT_ACCEPTANCE`, `REQUIREMENT_DECISION`, `DECISION_ARCHITECTURE`, `ARCHITECTURE_CONTRACT`, `CONTRACT_TASK`, `TASK_ATTEMPT`, `TASK_LEASE`, `ATTEMPT_CHECKPOINT`, `ATTEMPT_EXECUTION`, `LEASE_CHANGESET`, `CHANGESET_EXECUTION`, `EXECUTION_ENVIRONMENT`, `EXECUTION_EVIDENCE`, `EVIDENCE_REVIEW`, `REVIEW_VALIDATION`, `VALIDATION_ENVIRONMENT`, `VALIDATION_CERTIFICATION`.

Orphan detection is a deterministic SQLite query/service operation, not an LLM judgment.

The canonical admission query is: any integrated changeset whose latest `INTEGRATION_ADMISSION` verdict is not `ADMITTED`, or which has no admission record at all, is an orphan and blocks certification.

### Long-running execution provenance
The existing trace graph now includes reliability links for `TASK_ATTEMPT`, `ATTEMPT_CHECKPOINT`, `ATTEMPT_EXECUTION`, `EXECUTION_ENVIRONMENT`, and `VALIDATION_ENVIRONMENT`. These connect the canonical TaskAttempt, workspace checkpoint, execution, environment and validation records without creating a second source of truth. Trace history remains append-oriented.
