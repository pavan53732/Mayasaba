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

### Implemented durable ordering

The current runtime enforces this sequence for a material command:

1. Persist execution as APPROVED.
2. Persist APPROVED → STARTING.
3. Re-check the TaskAttempt fence immediately before spawn when the execution is attempt-bound.
4. Apply inherited-environment scrub keys, then explicit adapter-provided environment.
5. Spawn the local process and persist its PID as an EXPECTED process observation.
6. Persist STARTING → RUNNING.
7. Drain stdout/stderr concurrently into bounded buffers; truncation is explicit.
8. On exit or timeout, persist a physical process observation and transition to EXITED or TIMEOUT.

A failure to persist after the OS side effect triggers an owned-tree termination attempt rather than silently continuing.
A launch failure before a process exists leaves STARTING for recovery; it is not mislabeled as CRASHED.

### Current Windows termination boundary

Windows process containment now uses an atomic Job Object ownership boundary. The child is created suspended,
assigned to a private Job Object configured with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, and only then is its primary
thread resumed. Timeout, cancellation and post-spawn durability failure terminate the Job Object rather than a
shell utility. Descendants created by the contained process remain in the job by default. If containment setup
or terminal process observation fails, recovery preserves UNKNOWN rather than claiming success.


## Reliability refinements

### Process-tree termination boundary
Every execution records a physical process observation. On Windows, the kernel creates the child suspended, assigns it to a
private Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, and resumes it only after assignment succeeds.
Termination uses TerminateJobObject. Completion still requires observation; if physical state cannot be established,
recovery records UNKNOWN.

### Ports and local resources
Ports, process slots and other scarce machine-local resources are admitted through `ResourceReservation`. The execution kernel must not assume that a port is free because the reservation table says so; admission reconciles the reservation with actual OS/process state.

### Environment provenance
At execution/validation start, capture an `EnvironmentSnapshot` containing observed OS identity, relevant runtime/toolchain versions and the environment-policy hash. The snapshot ID is carried into execution evidence and later certification binding.

### Command provenance
Each command remains tied to project/task/attempt/session/workspace and causal identifiers. The kernel records the exact cwd and effective non-secret environment policy. Secret values are never copied into command evidence.

### Unknown physical state
If a process disappears before its terminal result is observed, the kernel records `UNKNOWN` rather than guessing success or failure. Reconciliation may later map the attempt to a terminal outcome after filesystem/process evidence is checked.
