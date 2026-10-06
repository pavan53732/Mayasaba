# Mayasaba Integration and Change Authority

## Purpose
Agents may implement independently, but Mayasaba controls how changes become authoritative project state.

## Workspace roles
Agent workspace: isolated workspace owned by an active task lease.
Integration workspace: controller-controlled workspace used for authoritative integration and validation.

## Integration flow
Agent implementation -> checkpoint/diff -> implementation report -> evidence collection -> integration eligibility checks -> controlled integration -> post-integration validation -> review/repair as required.

## Eligibility
A changeset must identify project, task, lease, agent/session, workspace, context snapshot, project epoch, changed files, diff/checkpoint/commit, tests/evidence, and applicable requirements/decisions/contracts.

Eligibility is a recorded decision, not a checklist that runs. WorkspaceService persists an admission record (`schemas/workspace-v1/admission.schema.json`) carrying those identifiers, a per-check status, and one verdict; integration proceeds only from `ADMITTED`. The record pins the epoch and context digest it was decided under, so a decision reused after the context changed is detectably stale.

## Merge authority
Agents do not make changes authoritative. Mayasaba controls integration ordering, conflict handling, integration checkpoints, post-integration validation, and rollback/recovery.

## Concurrent changes
Parallel integration is allowed only when dependency analysis and workspace isolation permit it. Conflicting or contract-sensitive changes are serialized or blocked.

## Conflict handling
Conflicts become explicit integration failures. They may require a repair task, handoff, or user decision. They are never silently overwritten. A refused integration is persisted as a `REFUSED` admission record carrying its reasons, so the failure is auditable and the changeset is not silently dropped.

The reasons are not free text. `schemas/workspace-v1/integration-conflict-classes.json` owns a closed vocabulary of conflict classes, and a refusal may cite only a class the controller can compute from facts it already records: `PATH_OVERLAP`, `PROTECTED_PATH`, `SCHEMA_OR_CONTRACT_FILE_CONFLICT`, `DEPENDENCY_MANIFEST_CONFLICT`, `STALE_BASE` and `POST_MERGE_VALIDATION_FAILURE`. Each class names those facts, and the contract gate fails if the vocabulary, the admission schema and the constants in `crates/storage` stop agreeing.

`NO_CONFLICT` is a member of the vocabulary because a classification has to be total — the controller must be able to say it looked and found nothing — and it is never a refusal reason, because a refusal citing it would say both that there is a conflict and that there is none. `TEXT_CONFLICT` is deliberately absent, and its absence is recorded with its reason rather than left to be noticed: nothing in this repository observes hunks or line ranges, so a textual conflict is a class the controller cannot compute and could therefore only be asserted.

## Checkpoints and rollback
Risky integration gets a recoverable checkpoint. Rollback returns to a known validated checkpoint and never erases historical events.

## Certification boundary
Only integrated, validated project state contributes to final certification.
