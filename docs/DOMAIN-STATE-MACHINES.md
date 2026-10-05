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
- DISCOVERY completes when the ProjectBrief baseline is established, objective local/workspace discovery has been performed, and the brief is judged complete enough to begin independent analysis. The brief version current at that point is the immutable analysis anchor for the lineage.
- USER_INTERVIEW is entered only when deliberation has produced a material unresolved question that survives the council question engine's normalization, deduplication, evidence-check and impact ranking. It is never entered as a general clarification gate; brief-level clarification belongs to DISCOVERY.
- These two gates are distinct in trigger, not merely in timing: DISCOVERY is driven by the user's own brief and objective discovery; USER_INTERVIEW is driven by agent-generated questions.

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

REQUESTED → ACTIVE ⇄ RENEWING → EXPIRED / RELEASED / REVOKED.

Expiry is authoritative from persisted timestamps and heartbeat policy. Expired/revoked leases cannot authorize writes.

## 5. Council round

Owner: CouncilService.

OPEN → RESPONSES_COLLECTING → CRITIQUE → REBUTTAL → REVISION → DISAGREEMENT_REVIEW → CLOSING → SEALED.

USER_INPUT_REQUIRED and NON_PARTICIPATION/TIMEOUT are explicit conditions/outcomes.

Termination is convergence-first with the round cap as a backstop. A round advances only while `round_index < max_rounds`; a round at the cap must close or escalate. The recorded `outcome_type` is exactly one of `CONVERGED`, `SYNTHESIZED`, `CAP_REACHED`, `ESCALATED` or `SEALED_WITH_OPEN_QUESTION`.

Convergence is a fixpoint test over the position set, never a count of agreeing agents. Silence never means agreement.

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

CURRENT → SUPERSEDED → INVALIDATED, with CURRENT → INVALIDATED as a direct branch.

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


## 17. State-machine ownership matrix

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

### 17.1 State machines that deliberately do not exist

Admission deliberately has no state machine. It is a per-evaluation decision record, not a lifecycle: each gate run produces one immutable `admissions` row with one verdict, and a re-evaluation appends a superseding row rather than transitioning the previous one. Modelling it as a machine would imply mutable gate state and a "current" admission that could be edited, which is exactly the property the record exists to prevent.

## Machine-readable transition authority

The complete transition registry is `schemas/mcf-v2/transition-types.json`. It contains explicit transition records with owner, source/target state, event/command, guards, authorization, required/forbidden fields, state mutation, emitted events, epoch effect, outbox effect, idempotency behavior, transaction boundary and failure transition.

Each record's `event_type` is a canonical MCF event, and every record for one transition names the same event. A transition's event is the canonical event for *entering* its target state, so the `ADVANCE_<machine>` spine records that satisfy the adjacency requirement above carry the same event as the edge's specific-command record rather than a generic `<MACHINE>_CHANGED` placeholder.

A machine declares its ordered path as `spine`, separately from its `states` set. `states` is the set of legal states and may include terminal or branch states that are not on the ordered path; `spine` is that path. The local gate enforces adjacency over `def.spine ?? def.states`, so a machine whose state set is not a single ordered path must declare a `spine`, or the check will require transitions out of its terminal states. Machines that omit `spine` (`project`, `council_round`) have state sets that are already one ordered path. The check requires a spine's edges to exist; it does not yet forbid a transition that is off-spine but not a declared branch, so an illegal edge that is merely present still passes. Only `message_delivery` has had its illegal off-spine edges removed; the other affected machines still carry theirs pending a declared `branches` set (DEC-038).

Each record's `command` is registered in `registry.json:transition_commands` with its machine, kind and owning service; the owning service is the one named in the ownership matrix above, and the crate it resolves to in `workspace.manifest.json` must equal the machine's owner here. Message delivery is the one machine with no application service — `crates/bus` owns it directly.

`crates/core` coordinates cross-machine transitions but cannot mutate a state machine owned by another crate.


## Reliability state semantics

`TaskAttempt` is not a second task lifecycle. It is a durable execution-attempt record whose state is subordinate to the canonical `task` and `execution` machines. The controller may create a new attempt without changing the acceptance identity of the Task.

`TaskLease.lease_version` is the fencing value. A material transition or side effect derived from an attempt is legal only when the presented lease version equals the currently authoritative lease version.

`ResourceReservation`, `WorkspaceRevision` and `EnvironmentSnapshot` are durable records with closed vocabularies, but they do not introduce controller lifecycle machines. Their status describes persisted facts used by the owning scheduler/workspace/execution/validation authority.

`UNKNOWN` is an admissible unresolved execution/attempt fact, never a success state. Positive gates must reject unknown, stale or contradictory physical state.


## TaskAttempt sub-lifecycle

`TaskAttempt` is subordinate to the canonical `Task` lifecycle. Its state exists to describe one concrete execution attempt and is not an alternate Task status.

Allowed transitions:

```text
CREATED -> STARTED
STARTED -> RUNNING | FAILED | CANCELLED | UNKNOWN
RUNNING -> CHECKPOINTED | COMPLETED | FAILED | TIMED_OUT | LOST | CANCELLED | UNKNOWN
CHECKPOINTED -> RUNNING | COMPLETED | FAILED | LOST | CANCELLED | UNKNOWN
```

Terminal attempt states are `COMPLETED`, `FAILED`, `TIMED_OUT`, `LOST`, `CANCELLED` and `UNKNOWN`. Recovery never reactivates a terminal attempt; it creates a new attempt for the same Task when policy permits retry/reassignment.

Every state transition is compare-and-swap against the expected persisted state. A stale controller/worker action therefore becomes a failed transition rather than an overwrite. `UNKNOWN` is an unresolved physical outcome and never satisfies a success gate.

`TaskAttempt.fence_token` must equal the lease version captured when the attempt was created. Material execution and heartbeats require that the fence still matches the live lease.
