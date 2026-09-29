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

Git projects:

- integration workspace
- per-agent isolated worktree/branch
- controlled merges/checkpoints

Non-Git projects:

- project-root sandboxing
- controlled agent scopes
- checkpoints before risky changes
- serialized integration where isolation cannot be guaranteed

## 6. Execution kernel

Every command passes:

~~~text
REQUESTED → POLICY_CHECK → APPROVED → STARTING → RUNNING → EXITED → EVIDENCE_CAPTURED → RECORDED
~~~

Failure paths include DENIED, TIMEOUT, CANCELED, CRASHED and CLEANUP.

Mayasaba records executable, arguments, working directory, timestamps, exit status and stdout/stderr evidence references.

## 7. Capability registry

Execution may use locally discovered:

PowerShell, CMD, Git, Node/npm/pnpm/yarn, Python, .NET/MSBuild, Java/Gradle/Android SDK, browsers/browser automation, database CLIs, compilers and package managers.

No command is invented merely because a framework is expected to have one; repository/toolchain evidence should determine the command.

## 8. Permission levels

READ_ONLY
SAFE_WRITE
PROJECT_WRITE
EXECUTE
INSTALL
ADMIN_REQUIRED

Default scope is the selected project root. Outside-root access requires explicit escalation.

## 9. Material action gate

Before any material state-changing action:

identity + session + project + workspace + capability + policy + task lease + current context/epoch + relevant requirements/decisions/contracts must be valid.


Detailed workspace lifecycle, Git worktree admission, integration, conflict handling and rollback are defined in `WORKSPACE-INTEGRATION-DESIGN.md`.

Detailed local process supervision and evidence capture are defined in `EXECUTION-KERNEL-DESIGN.md`.
## Execution authority

Agent file-editing is permitted only inside the leased workspace when the adapter capability probe confirms scoped file-write support.

Shell, process, package, install, compiler and administrative operations are not direct agent authority. They are requested through MCF `EXECUTION_REQUEST` and executed by Mayasaba `ExecutionService`.

An adapter must enforce or prove this separation. If the installed CLI cannot provide the required mediation, the session is not admitted for tasks that require privileged execution.
