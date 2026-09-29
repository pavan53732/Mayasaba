# Mayasaba Consolidated Design History

This document records the design decisions consolidated from the Mayasaba architecture work. It is a historical index, not a second source of technical truth.

## Baseline identity

Mayasaba is the canonical project name.

Repository:
pavan53732/Mayasaba

This repository is the single canonical implementation location for Mayasaba.

## Major decisions

### D-001 — Windows-only local-first product
Mayasaba executes on the user's Windows machine. Cloud workspaces and remote executors are outside the product boundary.

### D-002 — Four initial coding agents
The initial agent set is exactly Claude Code CLI, Hermes Agent CLI, Kilo Code CLI and Cline.

### D-003 — Mayasaba is a control plane
Mayasaba is not a fifth AI brain. Agent intelligence remains independent.

### D-004 — Council model
Agents independently analyze, then propose, critique, rebut, revise and resolve disagreements. Material unresolved decisions can require user input.

### D-005 — First-class Engineering Control Room
Chat is a first-class project interface showing the engineering timeline, council, decisions, tasks, builds, tests, repairs and evidence.

### D-006 — MCF-v2
All agent communication is brokered through the local Mayasaba Communication Fabric.

### D-007 — Durable communication
MCF-v2 uses at-least-once delivery, ACK/NACK, idempotency, retry, dead-letter, ordering, causal linkage, outbox/inbox and replay.

### D-008 — Context synchronization
Project epochs, immutable ContextPacks and state digests prevent stale agents from modifying current project truth.

### D-009 — Task leases
Tasks are owned through expiring leases and recoverable handoffs rather than informal assignment.

### D-010 — Workspace isolation
Git worktrees/branches are preferred; non-Git projects receive scoped filesystem control and checkpoints.

### D-011 — Evidence-backed engineering
Build, run, test, E2E, review, repair and certification are evidence-producing activities.

### D-012 — Deterministic completion
An agent cannot declare project completion. Mayasaba certifies completion only after objective gates pass.

### D-013 — Architecture ownership
Each subsystem has one canonical owner and one source of truth.

### D-014 — Machine-readable protocol
MCF-v2 is defined by versioned JSON Schema contracts, typed registries, transition manifests, fixtures and conformance tests.

### D-015 — Memory/design collision prevention
Future design changes must explicitly declare ADDITIVE, REFINEMENT, REPLACEMENT or DEPRECATION and identify ownership, compatibility and migration impact.

## Current canonical hierarchy

Master Specification
→ Architecture
→ Requirements
→ MCF-v2 Protocol
→ MCF-v2 Machine-Readable Contract
→ subsystem specifications
→ implementation
→ tests/evidence

Detailed technical definitions belong in their canonical subsystem documents, not in this history.
