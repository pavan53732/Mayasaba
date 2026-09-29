# Mayasaba Requirements-to-Evidence Traceability

## Purpose
Every material requirement must be traceable through design, implementation, validation, and certification.

## Canonical chain
USER INTENT -> REQUIREMENT -> ACCEPTANCE CRITERIA -> DECISION / HARD_LOCK -> ARCHITECTURE / CONTRACT -> TASK -> TASK LEASE / AGENT -> CHANGESET / ARTIFACT -> EXECUTION / TEST -> EVIDENCE -> REVIEW -> VALIDATION -> CERTIFICATION

## Traceability references
Requirements may link to acceptance criteria, decisions, architecture artifacts, contracts, tasks, commits/changesets, executions, tests, evidence, reviews, and validation runs.

## Coverage
NOT_ADDRESSED -> ADDRESSED -> IMPLEMENTED -> VALIDATED -> CERTIFIED

Implemented does not mean validated.

## Orphan detection
Detect requirements without tasks, tasks without requirements or explicit rationale, implementations without a valid task lease, changed files without an owning task, validations without scoped requirements, and certification claims without required evidence.

## Change impact
Changes to requirements, decisions, or contracts identify affected tasks, contexts, validations, and evidence and advance the project epoch according to the canonical context rules.

## Authority
Traceability indexes relationships between authoritative objects. It is not a second source of truth.
