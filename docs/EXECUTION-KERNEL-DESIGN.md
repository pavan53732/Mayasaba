# Mayasaba Local Execution Kernel Design

## Authority
This document defines local process execution mechanics. Policy and workspace contracts remain authoritative for permission and scope.

## Pipeline
```
REQUESTED
→ POLICY_CHECK
→ SCOPE_CHECK
→ CAPABILITY_CHECK
→ APPROVAL if required
→ STARTING
→ RUNNING
→ EXITED / TIMEOUT / CANCELED / CRASHED
→ CLEANUP
→ EVIDENCE_CAPTURED
→ RECORDED
```

## Process model
Every execution receives an execution ID and records executable, arguments, working directory, environment policy, parent/session/task IDs and timing.

Child processes are tracked. Cancellation escalates deterministically from graceful interruption to termination according to policy.

A stop is not complete until actual process state is verified.

## Environment
Environment variables are assembled from an explicit allow/deny policy. Secrets are not copied into ordinary logs or evidence.

PATH/tool discovery uses actual local runtime discovery.

## Scope
The default working directory and filesystem access are the user-selected workspace and task-scoped allowed paths. Mayasaba does not scan the whole PC by default. Outside-scope access or writes require explicit approval.

For user-requested research only, `READ_ONLY` access may retrieve publicly accessible sources; reports/citations are stored locally. It does not grant login, form submission, messaging, posting, purchase, account-change or unrelated-app automation.

## Command classes
READ_ONLY, SAFE_WRITE, PROJECT_WRITE, EXECUTE, INSTALL, ADMIN_REQUIRED.

Classification is recorded with the execution and retains the existing MCF-v2 enumeration. Project-local installs follow the authorized task policy; global/admin installs require approval. Agent-native capabilities do not bypass policy.

## Timeouts
Each execution has a configured timeout and cancellation grace period. Timeout produces durable evidence and a failure packet; it does not silently count as success.

## Output
stdout/stderr are streamed to bounded buffers and durable evidence references. Large output is stored as artifacts rather than embedded in protocol messages.

## Cleanup
Cleanup verifies process tree state and records survivors. Orphaned processes block completion of the affected execution until reconciled.

## Security
The UI cannot directly spawn processes. Rust execution services are the only privileged execution path.

## Evidence
Execution evidence contains command metadata, result, exit status, timestamps, output references, workspace identity and correlation/task IDs.

## Recovery
After controller restart, running executions are reconciled against actual OS process state before they can be resumed, canceled or marked failed.
