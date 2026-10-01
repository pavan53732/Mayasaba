# Mayasaba Local Security and Recovery

## Security boundary

Mayasaba's orchestration, workspace access and execution operate on the local Windows machine. There is no required Mayasaba cloud backend or remote executor.

For a user-requested research task, outbound retrieval from publicly accessible web sources may be used in read-only mode, with the report and citations saved locally. This does not authorize login, form submission, messages, posting, purchases, account changes, or control of unrelated applications. Agent-native tools do not widen this boundary.

## Least privilege

Capabilities are explicitly classified:

READ_ONLY
SAFE_WRITE
PROJECT_WRITE
EXECUTE
INSTALL
ADMIN_REQUIRED

`READ_ONLY` may cover public web retrieval only for a user-requested research task, limited to publicly accessible sources and no external state changes.

The user-selected workspace and task-scoped allowed paths define the default filesystem scope. Mayasaba does not scan the whole PC by default; outside-scope access/writes require explicit approval.

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

ADMIN_REQUIRED operations and writes outside the selected workspace/task scope require explicit approval. Project-local installs may proceed under the authorized project policy; global installs/admin actions require approval. Bulk or destructive data/file changes require a preview and recoverable checkpoint where feasible.

## External-action denial

Do not expose general automation of unrelated applications or actions that change external state. This includes sending email/messages, posting, form submissions, purchases and account changes. Public-web retrieval is limited to user-requested, read-only research on publicly accessible sources. Policy enforcement must apply even when an agent CLI has broader native capabilities.

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
- no whole-PC scan by default
- no external side-effect action
- no general control of unrelated applications

## Execution mediation invariant

Native agent terminal/shell capabilities do not grant independent execution authority.

For any privileged command:

`agent -> EXECUTION_REQUEST (MCF-v2) -> PolicyService -> ExecutionService -> process -> evidence -> EXECUTION_RESULT`

The adapter may keep scoped file-edit capabilities native to the CLI, but must prevent an uncontrolled native command path for tasks that require controller-mediated execution.
