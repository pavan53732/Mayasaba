# Mayasaba Task, Workspace and Execution Architecture

## 1. Task DAG

Requirements become acceptance criteria and then dependency-aware tasks.

A task includes:

- objective
- scope
- dependencies
- requirement refs
- decision/contract refs
- allowed paths
- allowed capabilities
- owner
- risk
- priority
- validation requirements
- workspace identity

Ready tasks may run in parallel when dependencies and isolation permit.

## 2. Task state

~~~text
PLANNED → READY → LEASE_REQUESTED → LEASED → ACCEPTED → IN_PROGRESS → REVIEW_PENDING → VALIDATION_PENDING → PASSED → COMPLETED
~~~

Recovery paths include BLOCKED, FAILED, RETRY_PENDING, REPAIR_PENDING, LEASE_EXPIRED, RECOVERY_PENDING, REASSIGNED and INVALIDATED.

## 3. Task lease

A lease contains:

- lease ID
- project/task
- owner agent/session
- workspace
- project epoch
- context snapshot
- allowed paths/capabilities
- issue/expiry timestamps
- heartbeat interval
- required validation
- lease version

Only the active owner may use the lease.

Expired leases cannot authorize writes.

## 4. Handoffs

A handoff is proof-carrying. It includes work completed, pending work, changed files/diff, checkpoint/commit, tests, failures, evidence, affected decisions/contracts, risks and current context/state digest.

~~~text
REQUESTED → PACKAGE_BUILT → OFFERED → RECEIVER_ACCEPTED → OWNERSHIP_TRANSFERRED → VERIFIED → CLOSED
~~~

## 5. Workspace model

A workspace is a user-selected local folder plus task-scoped allowed paths. It may contain code, documents, research reports, datasets or other user-selected artifacts; it is not limited to Git repositories or particular application categories. Mayasaba does not scan the whole PC by default. Outside-scope reads/writes require explicit approval.

Git software projects:

- integration workspace
- per-agent isolated worktree/branch
- controlled merges/checkpoints

Non-Git and file-focused workspaces:

- scoped filesystem isolation
- controlled agent scopes
- checkpoints before risky changes
- serialized integration where isolation cannot be guaranteed
- preview and recoverable checkpoint for bulk/destructive data edits where feasible

## 6. Execution kernel

Every command passes:

~~~text
REQUESTED → POLICY_CHECK → APPROVED → STARTING → RUNNING → EXITED → EVIDENCE_CAPTURED → RECORDED
~~~

Failure paths include DENIED, TIMEOUT, CANCELED, CRASHED and CLEANUP.

Mayasaba records executable, arguments, working directory, timestamps, exit status and stdout/stderr evidence references.

## 7. Capability registry

Execution may use locally discovered:

PowerShell, CMD, Git, Node/npm/pnpm/yarn, Python, .NET/MSBuild, Java/Gradle/Android SDK, database CLIs, compilers and package managers. User-requested research may additionally use read-only retrieval/rendering of publicly accessible web pages; this does not permit authenticated interaction with public/external services, public form submissions, posting, messaging, account changes, purchases, or general control of unrelated apps. Scoped UI automation may still validate the selected local target app for software tasks when it causes no external side effects.

No command is invented merely because a framework is expected to have one; repository/toolchain evidence should determine the command.

## 8. Permission levels

READ_ONLY
SAFE_WRITE
PROJECT_WRITE
EXECUTE
INSTALL
ADMIN_REQUIRED

Public-web research uses `READ_ONLY` scoped to user-requested access to publicly available sources; it grants no external side effects.

Default filesystem scope is the user-selected workspace and task allowed paths. Outside-scope access requires explicit approval. Project-local installs are allowed within the authorized task policy; global/admin installs require approval.

## 9. Material action gate

Before any material state-changing action:

identity + session + project + workspace + capability + policy + task lease + current context/epoch + relevant requirements/decisions/contracts must be valid.


Detailed workspace lifecycle, Git worktree admission, integration, conflict handling and rollback are defined in `WORKSPACE-INTEGRATION-DESIGN.md`.

Detailed local process supervision and evidence capture are defined in `EXECUTION-KERNEL-DESIGN.md`.
## Execution authority

Agent file-editing is permitted only inside the leased workspace when the adapter capability probe confirms scoped file-write support.

Shell, process, package, install, compiler and administrative operations are not direct agent authority. They are requested through MCF `EXECUTION_REQUEST` and executed by Mayasaba `ExecutionService`.

An adapter must enforce or prove this separation. If the installed CLI cannot provide the required mediation, the session is not admitted for tasks that require privileged execution.

Agent-native capabilities do not authorize external side-effect actions or general control of unrelated applications. Email/messages, online posting, public/external form submission, purchases and account changes are outside product scope; permitted public-web access is limited to user-requested read-only research.


## Execution reliability refinements

### TaskAttempt
`Task` is the acceptance-bearing unit; `TaskAttempt` records one concrete attempt to satisfy it. Retries never rewrite the original task identity. `attempt_no` is unique per task and `attempt_id` is globally unique.

An attempt binds task, lease, agent/session, workspace, project epoch and context snapshot. The attempt's `fence_token` equals the lease's `lease_version`. A worker whose token is no longer current may observe state but cannot perform a material write.

### Safe points
Long-running execution should persist a safe point before pause, risky mutation, migration or worker drain. The safe point is an existing `WorkspaceCheckpoint` of kind `SAFE_POINT`, accompanied by the attempt's latest evidence/state digest. Safe-point creation is not a claim that the task is complete.

### Plan and context freshness
An attempt may start only from a currently admitted plan/context tuple. A material change to requirements, decisions, contracts or architecture invalidates affected plans and context snapshots. Returning work from an older epoch produces `STALE_RESULT` and requires recomputation or explicit repair.

### Mutation scope
Task admission must carry allowed paths and capabilities. Optional deny-lists for forbidden paths/tools/commands may further narrow the scope. Scope violations are policy failures and never become implicit expanded authority.

### Resource admission
Before a lease is activated, scheduler-controlled reservations are acquired for required local resources. A resource is not considered available merely because a previous owner has not yet reported release; observed process/port state must agree with the durable reservation.

### Dependency cancellation
A failed dependency blocks downstream tasks rather than manufacturing downstream failures. Repair of the dependency reopens only the affected dependents whose preconditions become true again.


An attempt binds task, lease, agent/session, workspace, project epoch and context snapshot. Its relationship to command executions, checkpoints and admissions is recorded through the existing traceability model; the attempt's `fence_token` equals the lease's `lease_version`.
