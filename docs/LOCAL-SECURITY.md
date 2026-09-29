# Mayasaba Local Security and Recovery

## Security boundary

Mayasaba operates entirely on the local Windows machine for orchestration and execution.

There is no required Mayasaba cloud backend or remote executor.

## Least privilege

Capabilities are explicitly classified:

READ_ONLY
SAFE_WRITE
PROJECT_WRITE
EXECUTE
INSTALL
ADMIN_REQUIRED

The selected project root is the default filesystem scope.

## Agent identity

Every agent process/session is bound to:

- project
- session
- agent type
- workspace
- adapter
- authenticated/readiness state

Identity must be validated before material actions.

## Secret handling

API keys, access tokens, passwords and other secrets are not copied into council messages, normal logs, evidence or model-shared context.

Use the owning CLI's authentication/configuration where possible.

## Command safety

Every command must be:

- policy checked
- scoped to a workspace
- recorded
- cancellable
- timeout protected
- cleaned up after abnormal termination

ADMIN_REQUIRED or destructive operations require explicit approval unless a project policy explicitly authorizes them.

## Project isolation

Every message, session, event, artifact and task is project-scoped. Cross-project routing is rejected.

## Process recovery

On process crash:

1. record failure
2. reconcile child processes
3. preserve checkpoint/worktree
4. reconcile leases
5. verify actual workspace state
6. reconnect/reassign
7. regenerate current context
8. resume only after action gates pass

## Restart recovery

On Mayasaba restart:

1. load durable project state
2. reconcile unfinished messages/outbox
3. inspect inbox/deduplication
4. reconstruct sessions
5. verify CLI processes
6. reconcile leases
7. verify workspaces
8. detect stale contexts
9. rehydrate agents
10. resume only after required gates

## Security invariants

- no hidden commands
- no default network server
- no cloud execution
- no cross-project messages
- no secret leakage
- no stale-context writes
- no expired-lease writes
- no unscoped destructive actions

## Execution mediation invariant

Native agent terminal/shell capabilities do not grant independent execution authority.

For any privileged command:

`agent -> EXECUTION_REQUEST (MCF-v2) -> PolicyService -> ExecutionService -> process -> evidence -> EXECUTION_RESULT`

The adapter may keep scoped file-edit capabilities native to the CLI, but must prevent an uncontrolled native command path for tasks that require controller-mediated execution.
