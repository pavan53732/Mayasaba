# Mayasaba Workspace and Integration Design

## Authority
This document implements the workspace concepts defined by `INTEGRATION-AUTHORITY.md` and `TASK-EXECUTION.md`.

## Workspace classes
- integration workspace: controller-owned authoritative target
- agent workspace: isolated task-owned working scope
- checkpoint workspace/state: recovery reference

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
Only controller-integrated and validated state contributes to certification.
