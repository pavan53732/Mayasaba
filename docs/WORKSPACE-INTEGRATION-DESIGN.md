# Mayasaba Workspace and Integration Design

## Authority
This document implements the workspace concepts defined by `INTEGRATION-AUTHORITY.md` and `TASK-EXECUTION.md`.

## Workspace classes
- integration workspace: controller-owned authoritative target
- agent workspace: isolated task-owned working scope
- checkpoint workspace/state: recovery reference

## Workspace selection at project creation

At project creation the user selects one local Windows folder as the initial workspace root. Native folder selection is the primary interaction; a manually entered path and a user-selected folder are equivalent only after the same existence, locality and authorization checks succeed (DEC-048).

The root is the broadest project-level filesystem boundary. It does not automatically grant unrestricted access to every operation inside the folder. Task execution subsequently derives narrower task-scoped allowed paths from controller state, policy and task requirements.

## Git projects
Preferred structure:
```
project repository
├── integration branch/worktree
├── agent/<session-or-task> worktree
└── agent/<session-or-task> worktree
```
Agent branches/worktrees are disposable working scopes. The integration workspace is protected from direct agent writes.

## Admission
Before an agent receives a lease:
- repository/path identity verified
- clean/expected baseline established
- worktree assigned
- protected paths determined
- workspace lease bound to task and epoch

These checks are not a convention: they are recorded as an admission decision. WorkspaceService evaluates each check and persists exactly one `admissions` record of kind `WORKSPACE_ADMISSION` with a per-check status and a single `ADMITTED` / `REFUSED` / `BLOCKED` verdict, validated by `schemas/workspace-v1/admission.schema.json` and announced by `ADMISSION_RECORDED`. A lease is issued only against an `ADMITTED` record. A refusal is a durable artifact carrying its reasons, not a silent no-op.

## Change collection
A changeset records changed paths, diff/checkpoint/commit references, task/lease/session identity, epoch/context and validation evidence.

## Integration eligibility
Checks include:
- lease ownership valid
- context not stale
- workspace still belongs to task
- expected base/checkpoint valid
- required tests/evidence present
- no protected-path violation
- no unresolved dependency conflict

Eligibility is decided, not merely evaluated: the same record shape is persisted with kind `INTEGRATION_ADMISSION` before a changeset is integrated, naming the base checkpoint, the changed paths, the epoch and the context digest the decision was made under. Integration proceeds only from an `ADMITTED` record. Because the record pins the epoch and context digest, an admission taken under a context that has since gone stale is detectably stale rather than silently reused.

The schema is self-enforcing, so the two failure modes that matter most cannot be represented: an `ADMITTED` verdict cannot coexist with a `FAIL` check, and a `REFUSED` verdict must carry at least one refusal reason.

## Merge
Controller performs or authorizes merge. Conflicts create explicit integration failure state.

Before merge, create recoverable checkpoint. After merge, run affected validation.

## Rollback
Rollback targets a known checkpoint. It does not delete event/evidence history. The failed changeset remains auditable.

## Non-Git and file-focused workspaces
Use the user-selected local workspace and task-scoped allowed paths, checkpoints and serialized integration when concurrent writes cannot be proven safe. Mayasaba does not scan the whole PC by default; outside-scope reads/writes require explicit approval.

## Dirty/unexpected workspace
Unexpected changes are a blocker. Mayasaba must not silently overwrite or discard user/other-agent changes.

## Recovery
After crash/restart, re-read actual filesystem/Git state, reconcile checkpoint and lease identity, then either resume, isolate, rollback or block.

## Integration invariant
Only controller-integrated and validated state contributes to certification. Integration is reachable only through a persisted `INTEGRATION_ADMISSION` record whose verdict is `ADMITTED`, so "integrated" implies "admitted" and the certification boundary is auditable rather than assumed.


## Integration transaction model

Integration is a persisted protocol, not one ACID transaction across Git/filesystem/build/test. SQLite transactions make the controller's admission/state/evidence writes atomic; external workspace operations are reconciled separately.

Canonical integration protocol:

```text
ADMISSION
→ CHECKPOINT
→ PREPARE CHANGESET
→ APPLY/MERGE
→ OBSERVE WORKSPACE REVISION
→ BUILD/TEST AS APPLICABLE
→ VALIDATE
→ PROMOTE OR ROLLBACK
```

Failure during any external step leaves the changeset auditable and resolves through checkpoint restore, compensating revert, isolation or explicit human/recovery block. Event/evidence history is never rolled back.

### Workspace revisions and external change
Every integration boundary records a `WorkspaceRevision`. The controller compares the expected revision with the observed filesystem/Git state. A user/IDE/other-process modification that is not part of the admitted changeset creates a `DRIFTED` revision and blocks affected integration rather than overwriting it.

### Stale integration
An integration admission pins project epoch, context digest and base checkpoint. If any of those no longer match, the admission is stale and cannot be reused; a new admission must be evaluated.
