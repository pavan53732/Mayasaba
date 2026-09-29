# Mayasaba End-to-End System Design

## 1. Purpose

This document composes the existing Mayasaba subsystem contracts into one executable end-to-end design.

It does not replace subsystem specifications. Each subsystem remains authoritative for its own behavior.

Canonical sources:
- `docs/MASTER-SPECIFICATION.md`
- `docs/ARCHITECTURE.md`
- `docs/INTERNAL-APPLICATION-ARCHITECTURE.md`
- `docs/MCF-V2-PROTOCOL.md`
- `docs/MCF-V2-MACHINE-READABLE-CONTRACT.md`
- `docs/AGENT-ARCHITECTURE.md`
- `docs/AGENT-INTEGRATION.md`
- `docs/TASK-EXECUTION.md`
- `docs/INTEGRATION-AUTHORITY.md`
- `docs/VALIDATION-REPAIR.md`
- `docs/COUNCIL-ENGINE.md`
- `docs/MEMORY-CONTEXT.md`
- `docs/DATA-MODEL.md`
- `docs/LOCAL-SECURITY.md`
- `docs/BOOTSTRAP-DOCTOR.md`
- `docs/CONFORMANCE-AND-TESTING.md`

## 2. Architectural composition

The end-to-end system is:

```
User
 ↓
Control Room
 ↓
Tauri Bridge
 ↓
Rust Application Services
 ↓
Authoritative Domain State
 ↓
Orchestrator
 ├── Council
 ├── Requirements / Decisions / Architecture
 ├── Context
 ├── Tasks / Leases
 ├── Agents / Adapters
 ├── Workspace / Integration
 ├── Execution
 ├── Build / Test / E2E
 ├── Validation / Repair
 └── Evidence / Certification
 ↓
SQLite + artifact storage
```

The MCF-v2 bus is the canonical boundary for all agent-facing communication.

## 3. End-to-end invariant

At every material action:

```
authoritative state
+ current project epoch
+ current ContextPack
+ valid task lease
+ permitted workspace
+ runtime capability
+ policy authorization
+ applicable requirement/decision/contract
→ action may proceed
```

An agent response, progress message, or completion claim never overrides these facts.

## 4. Project creation and bootstrap

Flow:

```
CREATE PROJECT
 → choose local project path
 → initialize/open project record
 → establish project identity
 → run Doctor
 → detect available agents/tools
 → validate workspace
 → establish initial epoch/context
 → READY FOR DISCOVERY
```

Doctor results are persisted as structured evidence. Missing agents are explicit capacity reduction, not silent substitution.

## 5. Discovery and requirements

```
USER IDEA
 → capture intent
 → normalize objective
 → identify constraints
 → identify acceptance criteria
 → detect ambiguity
 → ask only unresolved material questions
 → persist requirements
 → establish traceability
 → create/update ContextPack
```

Requirements are versioned. Material requirement changes increment project epoch.

## 6. Council flow

Four available agents may independently analyze the objective.

```
INDEPENDENT_ANALYSIS
 → PROPOSALS
 → CROSS_CRITIQUE
 → COUNTERARGUMENT
 → REBUTTAL
 → REVISION
 → DISAGREEMENT_RESOLUTION
 → USER_QUESTION when required
 → DECISION_CANDIDATE
 → LOCK / OPEN
```

The council cannot silently manufacture consensus.

Material unresolved disagreement blocks the dependent gate or creates a user decision request according to the council contract.

## 7. Product, UX, and architecture

After sufficient requirements are resolved:

```
PRODUCT / UX DESIGN
 → TECH STACK DEBATE
 → ARCHITECTURE PROPOSALS
 → ARCHITECTURE REVIEW
 → CONTRACT / INTERFACE DEFINITION
 → DECISION REGISTER UPDATE
 → ARCHITECTURE LOCK
```

Architecture locks become inputs to task planning and implementation context.

## 8. Task planning

```
LOCKED REQUIREMENTS + ARCHITECTURE
 → acceptance criteria
 → task decomposition
 → dependency graph
 → validation requirements
 → workspace requirements
 → capability requirements
 → risk / priority
 → READY TASKS
```

A task is schedulable only when all mandatory dependencies and gates are satisfied.

## 9. Deterministic scheduling

The scheduler repeatedly evaluates:

```
project active?
epoch current?
dependencies satisfied?
task not invalidated?
required capabilities available?
workspace available?
policy permits?
no conflicting lease?
retry budget available?
validation prerequisites satisfied?
```

Eligible tasks are ranked by deterministic policy. Parallelism is allowed only where resource/workspace/semantic conflicts are absent.

## 10. Agent admission

For each selected task:

```
SELECT AGENT
 → verify session READY
 → verify runtime capabilities
 → verify workspace compatibility
 → create task lease
 → attach ContextPack
 → send MCF-v2 TASK
 → wait for transport ACK
 → wait for explicit task acceptance
```

ACK is receipt only. The task is not successful because ACK was received.

## 11. Agent implementation loop

```
TASK_ACCEPT
 → IN_PROGRESS
 → agent performs work in isolated workspace
 → progress / tool / artifact events
 → execution evidence captured
 → implementation report
 → handoff or review request
```

The agent cannot directly mutate authoritative lifecycle state.

## 12. Workspace and integration

Git:

```
integration workspace
       ↑
 controlled integration
       ↑
agent worktree / branch
```

The agent workspace is isolated. Integration requires eligibility checks, conflict analysis and checkpointing.

Non-Git projects use project-root scoping, checkpoints and serialized integration when concurrent isolation is unsafe.

Conflicts are explicit and never silently overwritten.

## 13. Execution pipeline

Every privileged/local command is mediated:

```
REQUEST
 → POLICY_CHECK
 → WORKSPACE_SCOPE
 → CAPABILITY_CHECK
 → APPROVAL if required
 → START
 → RUN
 → EXIT / TIMEOUT / CANCEL / CRASH
 → OUTPUT CAPTURE
 → EVIDENCE
 → RECORD
```

Execution is cancellable and process cleanup is verified.

## 14. Build and runtime verification

```
IMPLEMENTED CHANGE
 → BUILD_PLAN
 → BUILD
 → LAUNCH
 → RUNTIME_VERIFY
```

A process that starts successfully is not considered functionally correct until runtime verification passes.

## 15. Test and E2E pipeline

```
RUNTIME_VERIFY
 → TEST_PLAN
 → UNIT / INTEGRATION TESTS
 → E2E / UI TESTS where applicable
 → RESULTS RECORDED
```

Tests are linked to requirements and acceptance criteria wherever defined.

## 16. Review pipeline

Implementation is independently reviewed:

```
CHANGESET
 → CORRECTNESS REVIEW
 → ARCHITECTURE REVIEW
 → SECURITY REVIEW
 → TEST REVIEW
 → REQUIREMENTS COVERAGE
 → UNINTENDED-CHANGE CHECK
```

Findings create repair work or blockers.

## 17. Failure and repair

```
FAILURE
 → FINGERPRINT
 → FAILURE PACKET
 → DIAGNOSIS
 → REPAIR REQUEST
 → REPAIR ACTIVE
 → BUILD / TEST
 → REGRESSION CHECK
```

Repair terminates on:
- validated success
- bounded retry exhaustion
- repeated identical failure
- regression threshold
- policy block
- required user decision

Test weakening, deletion, or acceptance weakening is not a valid repair strategy without an explicit reviewed decision.

## 18. Context synchronization

Every material task action references:

- project epoch
- context snapshot ID
- state digest
- relevant requirement hashes
- decision/contract hashes

When a material fact changes:

```
MATERIAL CHANGE
 → increment epoch
 → invalidate affected context
 → block stale material actions
 → rebuild ContextPack
 → revalidate leases
 → resume/reassign as permitted
```

Recovery never trusts private agent memory as authoritative state.

## 19. Pause, stop, and cancellation

Pause:
- prevent new work
- preserve running state where safe
- stop dispatch of non-control traffic
- maintain durable state

Cancel:
- target a specific operation/task
- propagate cancellation
- verify process termination when applicable
- reconcile lease/workspace state

Stop:
- prioritize emergency control traffic
- quiesce project
- terminate/clean running processes
- persist recovery state
- require explicit resume

## 20. Recovery

Controller restart:

```
LOAD SQLITE
 → RECONCILE OUTBOX
 → RECONCILE INBOX
 → REBUILD MATERIALIZED STATE
 → VERIFY PROCESSES
 → VERIFY WORKSPACES
 → RECONCILE LEASES
 → REBUILD CONTEXT
 → REASSESS READY WORK
 → RESUME / REASSIGN / BLOCK
```

Agent/adapter crash follows the same principle for the affected scope.

Historical events are preserved.

## 21. Certification

Certification requires all applicable gates:

```
Requirements covered
+ architecture/contracts consistent
+ workspace integrity verified
+ build PASS
+ runtime PASS
+ tests PASS
+ E2E PASS where applicable
+ independent review PASS
+ repair/regression PASS
+ evidence complete
+ no unresolved blocking issues
 → controller certification
```

Only the Mayasaba controller can emit certification.

## 22. Packaging

After certification:

```
CERTIFIED
 → package project output
 → capture package metadata/hash
 → persist package evidence
 → PACKAGE
 → COMPLETE
```

Packaging does not erase development evidence or event history.

## 23. Global failure rules

At any stage, a failure must become an explicit durable state.

No failure may be hidden by:
- changing a status string
- dropping an event
- deleting evidence
- deleting a failing test
- silently reassigning ownership
- silently changing a decision
- silently modifying acceptance criteria
- silently broadening permissions
- silently switching to a remote/cloud runtime

## 24. Global traceability

Every material project action must be reconstructable through:

```
USER INTENT
 → REQUIREMENT
 → ACCEPTANCE CRITERIA
 → DECISION / HARD_LOCK
 → ARCHITECTURE / CONTRACT
 → TASK
 → LEASE / AGENT
 → CHANGESET / ARTIFACT
 → EXECUTION / TEST
 → EVIDENCE
 → REVIEW
 → VALIDATION
 → CERTIFICATION
```

Missing links are integrity defects.

## 25. End-to-end state gates

The controller may advance only when the current gate is satisfied by authoritative state and evidence.

Suggested gate categories:

- discovery complete
- requirement completeness
- council resolution
- architecture lock
- task graph valid
- implementation accepted
- integration valid
- build valid
- runtime valid
- test valid
- E2E valid
- review valid
- repair/regression valid
- evidence complete
- certification valid
- package valid

Each gate is fail-closed.

## 26. Implementation principle

This document is a composition layer.

When a subsystem needs behavior not specified here, the implementation must consult that subsystem's canonical specification. If the behavior is materially unspecified there too, treat it as an architecture gap and resolve it through design governance rather than inventing it in code.
