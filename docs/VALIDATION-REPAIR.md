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

Failure packets, diagnoses, repairs, reviews and controller certification use schemas under schemas/validation-v1/. Repair retry and anti-loop limits are defined by repair-policies.json.
