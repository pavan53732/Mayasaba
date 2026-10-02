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

## Checkpoints and rollback
Risky integration gets a recoverable checkpoint. Rollback returns to a known validated checkpoint and never erases historical events.

## Certification boundary
Only integrated, validated project state contributes to final certification.
