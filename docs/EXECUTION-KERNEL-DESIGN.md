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
Default working directory and filesystem access are the task workspace/project root. Outside-root access requires explicit policy escalation.

## Command classes
READ_ONLY, SAFE_WRITE, PROJECT_WRITE, EXECUTE, INSTALL, ADMIN_REQUIRED.

Classification is recorded with the execution.

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
