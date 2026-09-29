# Mayasaba Roadmap

## M0 — Repository and architecture foundation
- canonical repository
- documentation
- workspace layout
- governance

## M1 — MCF-v2 protocol
- canonical JSON Schemas
- message registry
- event registry
- transition registry
- fixtures
- compatibility rules

## M2 — Durable bus
- SQLite event log
- transactional outbox
- receiver inbox
- ACK/NACK
- retry/dedupe
- ordering
- backpressure
- dead letters
- replay

## M3 — Four agent adapters
- Claude Code CLI
- Hermes Agent CLI
- Kilo Code CLI
- Cline
- runtime discovery
- handshake
- capability negotiation

## M4 — Context synchronization
- project epochs
- snapshots/deltas
- state digests
- stale-context enforcement

## M5 — Tasks and handoffs
- task DAG
- task leases
- recovery
- proof-carrying handoffs

## M6 — Council engine
- independent analysis
- debate
- critique
- disagreement resolution
- user question engine
- barriers
- decision locks

## M7 — Workspace/execution
- project path
- Git worktrees
- local command execution
- process lifecycle
- permissions

## M8 — Build/test/E2E
- build discovery
- runtime launch
- deterministic tests
- browser/UI validation

## M9 — Failure/repair
- failure fingerprints
- diagnosis
- bounded repair
- regression

## M10 — Cross-agent review
- independent review
- severity
- evidence-backed findings

## M11 — Control Room
- chat
- council timeline
- tasks
- agents
- builds
- tests
- repairs
- evidence
- preview

## M12 — Autonomous end-to-end loop
- idea → working application

## M13 — Certification/package
- final validation
- certification
- MSI creation
