# Mayasaba Decision Register

This file is a human-readable register of currently locked design decisions. It is intentionally concise; detailed semantics live in the canonical subsystem documents.

| ID | Decision | Status |
|---|---|---|
| DEC-001 | Project name is Mayasaba | HARD_LOCK |
| DEC-002 | pavan53732/Mayasaba is the canonical implementation repository | HARD_LOCK |
| DEC-003 | Windows-only | HARD_LOCK |
| DEC-004 | Local-only execution; no cloud/remote executor | HARD_LOCK |
| DEC-005 | Initial supported agents are Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, Cline | HARD_LOCK |
| DEC-006 | Mayasaba is a control plane, not a fifth AI brain | HARD_LOCK |
| DEC-007 | Agent communication is brokered by MCF-v2 | HARD_LOCK |
| DEC-008 | MCF-v2 uses at-least-once delivery + idempotency | HARD_LOCK |
| DEC-009 | SQLite is Mayasaba's durable orchestration source of truth | HARD_LOCK |
| DEC-010 | Context uses project epochs/snapshots/digests | HARD_LOCK |
| DEC-011 | Tasks use leases and explicit ownership | HARD_LOCK |
| DEC-012 | Git worktrees are preferred for parallel agent isolation | HARD_LOCK |
| DEC-013 | Completion is evidence-backed and controller-certified | HARD_LOCK |
| DEC-014 | Desktop stack: Tauri 2 + React 19 + TypeScript + Vite + Tailwind/shadcn | HARD_LOCK |
| DEC-015 | Core stack: Rust + Tokio | HARD_LOCK |
| DEC-016 | User-facing distribution is MSI only | HARD_LOCK |
| DEC-017 | One canonical owner per subsystem; no shadow sources of truth | HARD_LOCK |
| DEC-018 | Protocol schemas use JSON Schema Draft 2020-12 | HARD_LOCK |
| DEC-019 | No private chain-of-thought in Control Room | HARD_LOCK |

## Change procedure

Changing a HARD_LOCK requires:

1. explicit change request
2. impact analysis
3. affected decision/contract identification
4. council review where material
5. new version/supersession record
6. migration/reconciliation plan
7. updated tests/evidence
