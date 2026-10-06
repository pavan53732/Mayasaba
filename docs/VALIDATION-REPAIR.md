# Mayasaba Validation, Review, Repair and Certification

## 1. Validation principle

Mayasaba treats work-completion claims as untrusted until observable local-artifact evidence and task-appropriate deterministic checks support them.

## 2. Validation lifecycle

~~~text
REQUESTED → COLLECTING_EVIDENCE → RUNNING_CHECKS → RESULTS_RECORDED → PASS / FAIL
~~~

## 3. Review

Important changes are independently reviewed.

Review dimensions:

- correctness
- architecture
- security
- tests
- maintainability
- requirements coverage
- unintended changes

Review findings become repair tasks.

## 4. Failure packet

A failure packet captures:

- failure class, from the closed taxonomy owned by `schemas/mcf-v2/error.schema.json`
- stable failure fingerprint
- command/execution
- exit code
- stdout/stderr
- stack trace/log references
- changed files/diff
- recent attempts
- relevant requirements
- architecture/contract references
- context/epoch
- environment facts

## 5. Diagnosis

Diagnosis records candidate root causes, selected cause, confidence when useful, evidence, repair scope and risk.

## 6. Repair loop

~~~text
FAILURE → DIAGNOSIS → REPAIR_REQUEST → REPAIR_ACTIVE → RUN_APPLICABLE_CHECKS (rebuild/retest for software where relevant) → REGRESSION_WHERE_REQUIRED → PASS or another bounded repair cycle
~~~

## 7. Anti-loop rules

Mayasaba must detect:

- identical failure fingerprints
- repeated identical repair attempts
- regressions caused by a repair
- acceptance-criteria weakening
- test deletion/disablement
- unrelated broad rewrites

Retries are bounded. Checkpoints enable rollback.

## 8. Cross-agent review

The implementing agent cannot be the sole certification authority.

High-risk changes may require multiple independent reviews.

## 9. Certification

Certification is emitted only by the Mayasaba controller.

Required evidence is derived from the task's acceptance criteria and includes applicable items:

- requirements and acceptance-criteria coverage
- architecture/contract consistency where applicable
- build, runtime and critical-workflow results for software tasks where required
- tests or other task-appropriate deterministic integrity checks
- E2E/UI result where applicable
- source citations/metadata for research reports
- format/schema checks, diff review, record counts, invariants and recoverability evidence for document/data tasks where applicable
- review result and resolved high-severity issues
- regression result where required
- requested package/artifact and evidence bundle

Software build, runtime and packaging evidence is not a universal gate for non-software local artifact work.

## 10. Completion

COMPLETE is unreachable by agent declaration. It is a controller-owned terminal state after all applicable validation gates are satisfied.


## Machine-readable validation contracts

Failure packets, diagnoses, repairs, reviews and controller certification use schemas under schemas/validation-v1/. Repair retry and anti-loop limits are defined by repair-policies.json; the contract gate checks that its budgets are positive whole numbers and that its rules against test deletion and acceptance weakening are still in force, because a threshold nothing reads is a threshold that is not in force.

### Failure classification and recovery actions

A failure packet's `category` is its **failure class**, and that vocabulary has exactly one owner: the `category` enum in `schemas/mcf-v2/error.schema.json` — the same closed set every code in `schemas/error-v1/registry.json` is classified under. `schemas/validation-v1/failure.schema.json` declares no class of its own and restates no member, and the gate fails if the two lists stop matching.

The class decides what the controller does about the failure. `schemas/validation-v1/failure-class-policies.json` maps each class to one recovery action, and the actions are operation ids that `schemas/service-contracts-v1/registry.json` already declares rather than a second verb vocabulary, so a mapping cannot name an operation that does not exist. `schemas/recovery-v1/recovery.schema.json` records those actions and the outcome each reached, and the gate requires the two files to name the same actions: an action the record cannot express is unreachable, and an action no class maps to exists only to look complete.

Classification is computed by the controller from observed facts — the packet, the exit code and the registry entry — and never from an agent's self-report. A diagnosis record proposes candidate causes and actions (`diagnosis.schema.json:proposed_actions`); the controller decides.

A failure the controller cannot classify is recorded as `UNKNOWN`. `UNKNOWN` is deliberately not a member of the class enum and is not a class: it is the explicit statement that none was determined. It is never treated as success, never silently retried and never counted as a pass (DEC-083).


## Certification provenance and regression locks

Validation is binding-specific. A passing validation result is not a timeless statement about a project; it is a statement about an observed artifact/workspace/environment under a particular validator and test-suite version.

Every certification record must bind, where applicable:
- validation run
- exact artifact content hashes
- exact workspace revision
- exact environment snapshot
- validator implementation/version
- test-suite/version
- applicable requirement/decision/contract epoch

Any load-bearing binding change supersedes the certification. Historical certification remains immutable for audit.

### Regression locks
Capabilities and high-risk fixes may attach regression suites through the existing traceability/task validation model. Any integration touching a capability's affected dependency set reruns the registered regression checks before promotion.

### Evidence admissibility
Evidence is not ordered by one universal truth ladder. Admissibility is claim-specific: filesystem observations prove different facts from compiler output, runtime probes, source citations or architecture decisions. A validator may accept only the evidence kinds required by the task's acceptance contract.

### Repair convergence
Repair consumes a bounded budget. Repeated identical fingerprints, repeated no-op repairs or repeated regressions cause escalation/blocking instead of another unconstrained agent loop.


### CertificationBinding state semantics
`ASSERTED` is an immutable certification claim. `INVALIDATED` and `EXPIRED` are append-only supersession records that carry `supersedes_binding_id`; the previous row is never updated. The effective certification is the terminal row of the supersession chain, resolved by the validation/storage owner.
