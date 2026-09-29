# Mayasaba Requirements

## Functional requirements

### FR-001 Project creation
The user can create a project, select a local Windows path, and start the Mayasaba lifecycle.

### FR-002 Agent preflight
Mayasaba detects the four supported CLIs, versions, executable paths, authentication/readiness, working-directory support, transports and capabilities at runtime.

### FR-003 Independent analysis
Each available agent can independently analyze the project idea before seeing other agents' proposals.

### FR-004 Council deliberation
Agents can propose, question, critique, rebut, revise, agree, disagree, block, accept, reject and abstain through MCF-v2.

### FR-005 Targeted interview
Mayasaba clusters and deduplicates agent questions, removes questions answerable from evidence, and asks the user only material unresolved questions.

### FR-006 Decision locks
Requirements, architecture, contracts and other material decisions are versioned. HARD_LOCK cannot be silently changed.

### FR-007 Task planning
Requirements and acceptance criteria are transformed into a dependency-aware task DAG.

### FR-008 Agent assignment
Tasks are assigned through leases with explicit owner, workspace, capabilities, context and expiry.

### FR-009 Workspace isolation
Concurrent agent work is isolated using Git worktrees/branches where possible, or controlled filesystem checkpoints otherwise.

### FR-010 Local execution
Mayasaba executes commands/processes locally with policy enforcement, timeouts, cancellation and evidence capture.

### FR-011 Build/run/test
Mayasaba discovers and executes project-appropriate build, launch, test and E2E/UI workflows using observed repository/toolchain facts.

### FR-012 Evidence
Material implementation and execution results produce evidence and artifact references.

### FR-013 Cross-agent review
Implementing agents cannot self-certify important changes; independent review creates actionable findings.

### FR-014 Repair loop
Failures are fingerprinted, diagnosed, assigned for repair and retested with bounded retries and regression checks.

### FR-015 Recovery
Mayasaba survives UI/controller restart, individual CLI crashes, adapter restart, duplicate messages, delayed delivery and stale contexts without losing authoritative project state.

### FR-016 Control
User can pause, resume, retry, reassign, reopen explicit decisions, approve blocked operations and stop execution.

### FR-017 Certification
Only the controller can certify the project complete after objective gates pass.

## Non-functional requirements

### NFR-001 Determinism
Orchestration transitions, policy, task leasing, validation and state mutation are deterministic and machine-checkable.

### NFR-002 Auditability
Material state transitions and communications are durable, causally traceable and replayable.

### NFR-003 Isolation
Project-scoped data and agent sessions cannot cross project boundaries.

### NFR-004 Reliability
Communication uses at-least-once delivery, deduplication, bounded retry, dead-letter and recovery semantics.

### NFR-005 Responsiveness
Emergency control traffic must not be starved by model streaming or bulk telemetry.

### NFR-006 Security
Secrets remain out of normal messages/logs/evidence. Permissions use least privilege and project-root scoping.

### NFR-007 Compatibility
MCF-v2 uses explicit schema/version compatibility. Agent adapters isolate native CLI changes from the canonical protocol.

### NFR-008 Offline/local operation
Core operation requires no Mayasaba cloud service.

### NFR-009 Evidence-backed correctness
Completion claims must map to observable files, commands, outputs, tests and artifacts.

## Acceptance principle

A requirement is not “implemented” because source text exists. It is implemented only when the required behavior is validated with appropriate evidence.
