# Mayasaba Requirements

## Functional requirements

### FR-001 Project/workspace creation
The user can create a workspace, select a local Windows path, describe a software or local-artifact task, and start the applicable Mayasaba workflow. The described intent is persisted as the first version of a `ProjectBrief` and is project truth, not a chat message.

### FR-001a Project intent and brief versioning
The user's stated project intent is versioned and immutable per version. The brief version current when DISCOVERY closes is the analysis anchor for that lineage; later versions do not rewrite the historical meaning of snapshots or council rounds that consumed an earlier version.

### FR-001b Post-intake user contribution
The user can submit free-text input after project creation. Each submission is recorded as a `UserContribution` with an advisory classification. Only the owning authoritative service determines whether it materially changes project truth; a material change increments the project epoch, a non-material context change produces a new snapshot/digest at the current epoch, and commentary changes nothing. Free-text input never bypasses the existing mediated commands.

### FR-002 Agent preflight
Mayasaba detects the three supported CLIs, versions, executable paths, authentication/readiness, working-directory support, transports and capabilities at runtime.

### FR-003 Independent analysis
Each available agent can independently analyze the user request and scoped workspace facts before seeing other agents' proposals.

### FR-004 Council deliberation
Agents can propose, question, critique, rebut, revise, agree, disagree, block, accept, reject, abstain and synthesize through MCF-v2. Deliberation terminates on convergence of the position set, with a bounded round cap as a backstop; the round records exactly one termination outcome.

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

### FR-011 Software build/run/test
For software-engineering tasks, Mayasaba discovers and executes applicable build, launch, test and E2E/UI workflows using observed repository/toolchain facts.

### FR-012 Evidence
Material task changes and execution results produce evidence and artifact references.

### FR-013 Cross-agent review
Implementing agents cannot self-certify important changes; independent review creates actionable findings.

### FR-014 Repair loop
Failures are fingerprinted, diagnosed, assigned for repair and retested with bounded retries and regression checks.

### FR-015 Recovery
Mayasaba survives UI/controller restart, individual CLI crashes, adapter restart, duplicate messages, delayed delivery and stale contexts without losing authoritative project state.

### FR-016 Control
User can pause, resume, retry, reassign, reopen explicit decisions, approve permitted in-scope operations that require escalation, and stop execution. Approval cannot authorize external side-effect actions or general control of unrelated applications.

### FR-017 Certification
Only the controller can certify the requested work complete after its applicable objective gates pass.

### FR-018 Local artifact work
Mayasaba supports user-authorized tasks on files in the selected workspace, including documents, research reports, datasets/data cleanup and code refactors. Support is not restricted by application or file category; actual work and verification depend on available local tools and evidence.

### FR-019 Read-only public research
For a user-requested research task, Mayasaba may retrieve information from public web sources and save a report with citations/source metadata in the selected local workspace. Retrieval must not submit forms, authenticate, publish, send messages, make purchases, change accounts or otherwise cause external side effects.

### FR-020 External-action boundary
Mayasaba does not provide general control of unrelated applications or external side-effect actions such as sending email/messages, posting online, submitting forms to public/external services, purchasing, or changing accounts. Agent-native capabilities do not override this product boundary.

### FR-021 Scoped local file access
File reads and writes are limited to user-selected workspace/task scope. Mayasaba does not scan the entire PC by default; outside-scope access or writes require explicit approval.

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
Secrets remain out of normal messages/logs/evidence. Permissions use least privilege and user-selected workspace/task-path scoping. Public-web retrieval, when user-requested for research, is read-only; external side effects and general unrelated-app control are prohibited.

### NFR-007 Compatibility
MCF-v2 uses explicit schema/version compatibility. Agent adapters isolate native CLI changes from the canonical protocol.

### NFR-008 Offline/local operation
Core operation requires no Mayasaba cloud service.

### NFR-009 Evidence-backed correctness
Completion claims must map to observable artifacts and task-appropriate checks: files/diffs, commands and tests when relevant, citations for research, and integrity/invariant evidence for data transformations.

## Acceptance principle

A requirement is not “implemented” because source text exists. It is implemented only when the required behavior is validated with appropriate evidence.
