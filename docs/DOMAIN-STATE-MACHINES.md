# Mayasaba Domain State Machines

## Purpose

This document defines internal state machines and transition ownership. It is separate from MCF-v2 delivery states and from the UI's derived display states.

Each state machine has one authoritative owner.

The project-phase sequence below preserves the full software-engineering lifecycle. Other local artifact tasks use the same generic task/lease, validation and completion states, but enter only applicable project phases; build, E2E and software packaging are not universal gates.

## 1. Project lifecycle

Owner: LifecycleService / Orchestrator.

States:
PROJECT_CREATED, DISCOVERY, INDEPENDENT_ANALYSIS, PROPOSALS, CROSS_CRITIQUE, REBUTTAL_AND_REVISION, DISAGREEMENT_RESOLUTION, USER_INTERVIEW, PRODUCT_AND_UX_DESIGN, TECH_STACK_DEBATE, ARCHITECTURE_REVIEW, ARCHITECTURE_LOCKED, TASK_PLANNING, IMPLEMENTATION, INTEGRATION, BUILD, TEST, E2E, CROSS_AGENT_REVIEW, FINAL_VALIDATION, PACKAGE, COMPLETE.

Orthogonal conditions:
PAUSED, BLOCKED, FAILED, RECOVERING, STOPPING.

Rules:
- COMPLETE is controller-owned.
- an agent cannot advance lifecycle.
- a failed mandatory gate prevents advancement.
- BUILD, TEST, E2E and PACKAGE apply when required by the software task's acceptance criteria; they are not imposed on document, research or data tasks when irrelevant.
- task validation predicates are declared per task and are enforced by ValidationService.
- material reopening moves to the applicable prior phase and increments epoch when project truth changes.

## 2. Agent session

Owner: AgentService.

DISCOVERED → HANDSHAKING → CAPABILITY_VALIDATING → WORKSPACE_VALIDATING → READY → ACTIVE → PAUSED → DRAINING → STOPPED.

Failure:
READY/ACTIVE → LOST → RECONNECTING → SYNCING → READY/ACTIVE or FAILED.

A lease cannot be issued before READY.

## 3. Task

Owner: TaskService.

PLANNED → READY → LEASE_REQUESTED → LEASED → ACCEPTED → IN_PROGRESS → REVIEW_PENDING → VALIDATION_PENDING → PASSED → COMPLETED.

Recovery:
BLOCKED, FAILED, RETRY_PENDING, REPAIR_PENDING, LEASE_EXPIRED, RECOVERY_PENDING, REASSIGNED, INVALIDATED.

A task may leave a recovery state only when its guard conditions are satisfied.

## 4. Lease

Owner: TaskService.

REQUESTED → ACTIVE → RENEWING → EXPIRED / RELEASED / REVOKED.

Expiry is authoritative from persisted timestamps and heartbeat policy. Expired/revoked leases cannot authorize writes.

## 5. Council round

Owner: CouncilService.

OPEN → RESPONSES_COLLECTING → CRITIQUE → REBUTTAL → REVISION → DISAGREEMENT_REVIEW → CLOSING → SEALED.

USER_INPUT_REQUIRED and NON_PARTICIPATION/TIMEOUT are explicit conditions/outcomes.

Silence never means agreement.

## 6. Handoff

Owner: TaskService.

AgentService supplies session facts; TaskService owns handoff ownership state and persistence.

REQUESTED → PACKAGE_BUILT → OFFERED → RECEIVER_ACCEPTED → OWNERSHIP_TRANSFERRED → VERIFIED → CLOSED.

Rejected handoffs retain evidence and reason.

## 7. Execution

Owner: ExecutionService.

REQUESTED → POLICY_CHECK → APPROVED → STARTING → RUNNING → EXITED → EVIDENCE_CAPTURED → RECORDED.

Failure:
DENIED, TIMEOUT, CANCELED, CRASHED, CLEANUP_REQUIRED.

## 8. Validation

Owner: ValidationService.

REQUESTED → COLLECTING_EVIDENCE → RUNNING_CHECKS → RESULTS_RECORDED → PASS / FAIL.

A PASS requires all applicable checks to satisfy their declared predicates.

## 9. Repair

Owner: RepairService.

REQUESTED → DIAGNOSING → ACTIVE → VERIFYING → REGRESSION → RESOLVED / RETRY_PENDING / BLOCKED.

Repeated identical failure fingerprints and bounded repair budgets force escalation rather than infinite looping.

## 10. Context

Owner: ContextService.

CURRENT → SUPERSEDED / INVALIDATED.

A context becomes invalid when its project epoch or required state digest no longer matches authoritative state.

Context regeneration creates a new immutable snapshot; historical snapshots remain unchanged.

## 11. Message delivery

Owner: Bus.

CREATED → PERSISTED → QUEUED → DISPATCHED → RECEIVED → ACKED → PROCESSING → PROCESSED.

Failure:
RETRYING, EXPIRED, REJECTED, DEAD_LETTER.

ACK is receipt only.

## 12. Barrier

Owner: CouncilService.

The Orchestrator evaluates barrier predicates and schedules follow-up actions, but cannot mutate barrier state directly. CouncilService persists barrier state and emits barrier events.

OPEN → WAITING → SATISFIED or BLOCKED / TIMED_OUT.

The completion predicate is evaluated from durable facts and evidence.

## 13. Transition contract

Every transition must define:
- owner
- source state
- event/command
- guards
- authorization
- required fields
- forbidden fields
- state mutation
- emitted event
- epoch effect
- outbox effect
- idempotency behavior
- failure transition

## 14. Cross-machine rule

A transition in one machine cannot directly mutate another machine's state without invoking that machine's authoritative service/command.

Example:
an agent ACK cannot transition a task to COMPLETED.
A validation PASS can satisfy a task validation gate, but TaskService owns the task transition.
A UI click can request pause, but LifecycleService owns PAUSED.

## 15. Fail-closed rule

Unknown, missing, stale, contradictory or unverifiable state cannot satisfy a positive gate.

## 16. Recovery rule

Recovery reconstructs machine state from durable authoritative state and actual external state. It does not infer success from stale process/session messages.


## 13. State-machine ownership matrix

| State machine | Authoritative owner | Crate |
|---|---|---|
| Project lifecycle | LifecycleService / Orchestrator | crates/core |
| Agent session | AgentService | crates/agents |
| Task | TaskService | crates/tasks |
| Lease | TaskService | crates/tasks |
| Council round | CouncilService | crates/council |
| Handoff | TaskService | crates/tasks |
| Execution | ExecutionService | crates/execution |
| Validation | ValidationService | crates/validation |
| Repair | RepairService | crates/core |
| Context | ContextService | crates/core |
| Message delivery | Bus | crates/bus |
| Barrier | CouncilService | crates/council |

The Orchestrator coordinates these owners but is never a second owner. Cross-machine changes invoke the authoritative owner service.
## Machine-readable transition authority

The complete transition registry is `schemas/mcf-v2/transition-types.json`. It contains explicit transition records with owner, source/target state, event/command, guards, authorization, required/forbidden fields, state mutation, emitted events, epoch effect, outbox effect, idempotency behavior, transaction boundary and failure transition.

`crates/core` coordinates cross-machine transitions but cannot mutate a state machine owned by another crate.
