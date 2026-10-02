# Mayasaba Orchestrator Design

## Authority
This document is authoritative for the internal orchestration engine. It does not replace lifecycle, task, agent, council, context, protocol, validation, or persistence contracts; it defines how those authorities are coordinated.

## Responsibilities
The orchestrator:
- evaluates authoritative state
- applies lifecycle transition guards
- schedules eligible work
- coordinates barriers
- validates leases/context/capabilities/policy
- advances, blocks, retries, repairs, recovers and certifies
- never delegates authority to an agent

## Orchestration cycle
```
LOAD STATE
→ DETERMINE READY CONDITIONS
→ EVALUATE BLOCKERS
→ SELECT ACTIONS
→ OPEN TRANSACTION
→ APPLY AUTHORIZED DOMAIN TRANSITIONS
→ APPEND EVENTS + OUTBOX
→ COMMIT
→ DISPATCH
→ OBSERVE
→ VALIDATE RESULT
→ ADVANCE / BLOCK / RETRY / REPAIR / RECOVER
```

No external model response directly mutates authoritative state.

## Project lifecycle
```
PROJECT_CREATED
→ DISCOVERY
→ INDEPENDENT_ANALYSIS
→ PROPOSALS
→ CROSS_CRITIQUE
→ REBUTTAL_AND_REVISION
→ DISAGREEMENT_RESOLUTION
→ USER_INTERVIEW
→ PRODUCT_AND_UX_DESIGN
→ TECH_STACK_DEBATE
→ ARCHITECTURE_REVIEW
→ ARCHITECTURE_LOCKED
→ TASK_PLANNING
→ IMPLEMENTATION
→ INTEGRATION
→ BUILD
→ TEST
→ E2E
→ CROSS_AGENT_REVIEW
→ FINAL_VALIDATION
→ PACKAGE
→ COMPLETE
```
This is the full canonical software-engineering sequence. Other local artifact tasks use the shared task/workspace/council/validation authorities and enter only phases applicable to their acceptance criteria; they do not require irrelevant build, E2E or packaging stages.

Repair/recovery may re-enter the applicable phase. BLOCKED, PAUSED, FAILED and RECOVERING are orthogonal controller conditions.

## Intake

PROJECT_CREATED is entered from the Control Room intake surface, where the user states the idea and selects the local workspace. The stated intent is persisted as the first `ProjectBrief` version; it is project truth, not a chat message.

DISCOVERY then runs before any agent analyzes the brief. It establishes the brief baseline, performs objective local/workspace discovery, and judges whether the brief is complete enough to begin. The brief version current when DISCOVERY closes is the immutable analysis anchor delivered to every agent in the ContextPack for the resulting lineage.

A free-text user message submitted after creation does not enter the phase sequence directly. It is recorded as a `UserContribution`, routed by advisory classification to the owning service, and only a material truth change accepted by that service advances the epoch and invalidates affected contexts. No user message mutates authoritative state on its own.

## Ready-condition evaluation
A task/action is eligible only when:
1. project is active
2. dependencies are satisfied
3. required decision/contract/requirements are valid
4. epoch/context is current
5. required capability is available
6. workspace scope is available
7. policy permits action
8. no conflicting lease/resource exists
9. retry budget remains
10. required barrier is open

## Deterministic selection
Selection is deterministic for equal inputs. Inputs include priority, dependency readiness, risk, deadline/lease pressure, required capability, workspace conflicts, retry budget and configured fairness rules.

Model preference or persuasive agent text is never a scheduling authority.

## Parallelism
Parallelism requires:
- dependency independence
- non-overlapping write scopes or proven isolation
- no contract/architecture conflict
- no shared exclusive resource
- independent validation feasibility

Otherwise work is serialized.

## Barriers
A barrier has:
- ID
- project/round/task scope
- participants
- required outcomes
- timeout policy
- blocking policy
- completion predicate
- evidence requirement

Barrier completion is evaluated from persisted facts, not message count alone.

## Pause
Pause prevents new non-control admissions. Running operations are allowed to reach a safe checkpoint or are canceled according to policy. Control and recovery traffic remains available.

## Stop
Stop quiesces dispatch, cancels eligible operations, verifies process termination, reconciles leases/workspaces, persists recovery state and leaves the project resumable unless explicitly abandoned.

## Cancellation
Cancellation is scoped to project, task, execution, message or session. It is idempotent. A cancellation request is not considered complete until the affected resource reaches its terminal/canceled state or an explicit unresolved recovery state.

## Retry
Retries are bounded by operation-specific budgets. Retry identity and attempt number are durable. Permanent failures are not retried.

## Recovery
```
DETECT
→ RECORD
→ QUIESCE
→ VERIFY PROCESS
→ VERIFY WORKSPACE
→ RECONCILE BUS
→ RECONCILE LEASES
→ REBUILD CONTEXT
→ REASSESS
→ RESUME / REASSIGN / BLOCK
```

Recovery never erases historical events.

## Certification
Only the controller may transition into certification/completion. Certification requires the validation contract, evidence completeness, integration integrity and no unresolved blocking conditions.

## Crash consistency
Every authoritative transition is atomic with its event and required outbox records. After restart, the orchestrator reconstructs state from SQLite and resumes only after reconciliation gates pass.

## Failure semantics
Unknown state is not success. Missing evidence is not success. Agent claims are not validation. A failed gate blocks advancement.
