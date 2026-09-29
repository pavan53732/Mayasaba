# AGENTS.md — Mayasaba Repository Instructions for AI Coding Agents

## 1. Purpose

This file is the repository-level operating contract for any AI coding agent working inside the Mayasaba repository.

It tells an AI agent:
- what this repository is
- what architecture is authoritative
- how the repository must be modified
- how the agent must behave
- what must never be invented or bypassed
- how to validate changes
- where canonical documentation lives

These instructions apply to Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, Cline, and any future coding agent explicitly admitted to the project.

## 2. Repository identity

Repository: pavan53732/Mayasaba

Mayasaba is a Windows-only, local-first autonomous software-engineering control plane.

Initial supported coding agents:
1. Claude Code CLI
2. Hermes Agent CLI
3. Kilo Code CLI
4. Cline

Mayasaba is not a fifth AI brain.

## 3. Absolute product constraints

AI agents working in this repository MUST preserve:

- Windows-only target.
- Local execution on the user's Windows machine.
- No cloud VM or remote development executor.
- No mandatory Mayasaba cloud service/account.
- User-selected local project paths.
- MSI-only user-facing distribution.
- Tauri 2 + React 19 + TypeScript + Vite frontend baseline.
- Rust + Tokio controller/core baseline.
- SQLite as durable local source of truth.
- MCF-v2 as the sole canonical agent communication protocol.
- Evidence-backed completion.
- Controller-owned final certification.

Do not weaken, reinterpret, or silently replace these constraints.

## 4. Read the architecture before coding

Before making a material change, inspect the relevant canonical documents:

1. docs/MASTER-SPECIFICATION.md
2. docs/ARCHITECTURE.md
3. docs/INTERNAL-APPLICATION-ARCHITECTURE.md
4. docs/REQUIREMENTS.md
5. docs/MCF-V2-PROTOCOL.md
6. docs/MCF-V2-MACHINE-READABLE-CONTRACT.md
7. docs/DECISION-REGISTER.md
8. docs/DESIGN-GOVERNANCE.md
9. the subsystem document for the area being changed

Do not rely on a partial prompt when repository documentation can answer the question.

## 5. Source-of-truth hierarchy

When sources appear to conflict, use this hierarchy:

1. explicit current user requirement
2. current HARD_LOCK decision
3. machine-readable protocol/contract
4. Master Specification
5. canonical subsystem specification
6. implementation
7. agent assumptions or comments

If the conflict cannot be resolved safely, stop and report the architecture conflict instead of silently choosing.

## 6. Canonical ownership

Respect these subsystem owners:

- crates/core — lifecycle/orchestration
- crates/protocol — MCF-v2 types and protocol validation
- crates/bus — communication delivery/routing
- crates/agents — agent runtime/adapters
- crates/council — deliberation
- crates/tasks — task DAG and leases
- crates/workspace — workspaces/checkpoints/integration support
- crates/execution — local process execution
- crates/validation — validation gates
- crates/evidence — evidence/artifacts
- crates/policy — permissions and security policy
- crates/storage — SQLite persistence

Do not create a second authority for another subsystem.

## 7. MCF-v2 rule

All agent-to-Mayasaba and Mayasaba-to-agent communication MUST use MCF-v2.

Do not create:
- an ad-hoc agent message format
- a second bus protocol
- direct uncontrolled agent-to-agent communication
- UI-specific agent protocol semantics
- adapter-specific project-state semantics

Native CLI protocols belong inside adapters and must be translated to/from MCF-v2.

ACK means receipt, not successful execution.

## 8. Repository behavior

An AI agent MUST:

- inspect before editing
- preserve existing architecture unless a change is explicitly intended
- use the repository's canonical terms
- make minimal coherent changes
- keep documentation and implementation consistent
- preserve backward compatibility where required
- update schemas/contracts when behavior changes
- add or update tests for material behavior
- validate affected paths before claiming completion
- report uncertainty or blocked dependencies explicitly

An AI agent MUST NOT:

- invent missing APIs or commands without evidence
- silently change locked decisions
- bypass validation
- delete or weaken tests to obtain a pass
- hide failures
- fabricate successful execution
- claim a feature is implemented when only documentation exists
- introduce cloud/remote execution
- leak secrets
- modify unrelated areas without justification
- create duplicate architectures or competing sources of truth

## 9. Change classification

Every material architecture change is one of:

- ADDITIVE
- REFINEMENT
- REPLACEMENT
- DEPRECATION

For REPLACEMENT or DEPRECATION, document:
- previous behavior
- new behavior
- reason
- compatibility impact
- migration/reconciliation
- tests affected

## 10. Coding behavior

Before implementation:
- understand the existing module
- identify its canonical owner
- inspect interfaces/contracts
- inspect callers
- inspect tests
- identify invariants

During implementation:
- preserve typed contracts
- keep side effects behind the correct service boundary
- avoid global mutable shortcuts
- propagate correlation/request IDs
- use structured errors
- preserve cancellation
- make state transitions explicit

After implementation:
- format
- compile
- run focused tests
- run relevant integration tests
- inspect changed files
- verify no unrelated changes
- update documentation when behavior changed

## 11. Frontend rules

React is presentation and interaction state, not project truth.

Do not:
- create a second lifecycle state machine in React
- directly spawn processes
- directly access unrestricted filesystem paths
- directly mutate SQLite
- bypass Tauri/Rust policy gates

Use typed Tauri commands for requests and typed Tauri events for realtime state.

## 12. Rust/controller rules

Rust owns authoritative orchestration and privileged side effects.

Material mutations must pass through:
identity + project + workspace + capability + policy + task lease + current context/epoch.

Long-running operations must be cancellable and observable.

## 13. Agent behavior inside the repository

When an AI agent is implementing Mayasaba itself, it is a contributor to the system, not the system's authority.

The agent MUST NOT assume:
- its own answer is authoritative
- its own implementation is automatically correct
- its own completion message certifies work
- its private context overrides repository state

Agent work is accepted only after appropriate integration, tests, review, and validation.

## 14. Documentation behavior

Documentation is part of the product contract.

When discovering a design gap:
1. identify the canonical owner
2. update or create the correct canonical document
3. link it from docs/INDEX.md
4. update affected cross-references
5. classify the change
6. add implementation/test implications

Do not create a new document merely to duplicate an existing authority.

## 15. Security behavior

Never place API keys, tokens, passwords, private credentials, or secrets into:
- source code
- committed configuration
- tests
- documentation
- logs
- MCF messages
- evidence payloads

Use references to external/local credential stores where appropriate.

## 16. Validation behavior

A successful compile is not equivalent to a completed feature.

For material changes, validate the applicable chain:

requirement -> implementation -> build -> test -> evidence -> review -> validation.

For orchestration changes also test:
- duplicate events
- stale context
- lease expiry
- recovery
- cancellation
- failure paths

## 17. Git behavior

Before modifying:
- inspect status
- inspect relevant history when behavior is unclear
- avoid overwriting unrelated work

Do not discard another agent's changes.

Use isolated worktrees/branches where the Mayasaba workspace manager requires them.

## 18. Completion report

When reporting completed work, state:
- what changed
- files changed
- contracts affected
- tests run
- validation result
- known limitations
- remaining work

Never report "complete" merely because code was written.

## 19. If documentation and implementation disagree

Do not silently normalize the disagreement.

Determine whether it is:
- stale documentation
- stale implementation
- intentional pending migration
- architecture conflict

Then update the canonical source and affected dependents through the proper change process.

## 20. Final rule

When uncertain, prefer:
repository evidence > explicit contract > deterministic validation > assumption.

If a missing decision materially affects correctness, surface it as an architecture gap instead of inventing behavior.
