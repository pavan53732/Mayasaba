# Mayasaba Validation, Review, Repair and Certification

## 1. Validation principle

Mayasaba treats implementation claims as untrusted until observable evidence and deterministic checks support them.

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
FAILURE → DIAGNOSIS → REPAIR_REQUEST → REPAIR_ACTIVE → REBUILD/RETEST → REGRESSION → PASS or another bounded repair cycle
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

Required evidence includes:

- requirements coverage
- architecture/contract consistency
- build result
- runtime/critical workflow result
- test result
- E2E/UI result where applicable
- review result
- resolved high-severity issues
- regression result
- package/artifact
- evidence bundle

## 10. Completion

COMPLETE is unreachable by agent declaration. It is a controller-owned terminal state after all applicable validation gates are satisfied.
