# CLAUDE.md — Mayasaba Claude Code Instructions

## Purpose

This file defines the repository-local operating contract for **Claude Code CLI** when working inside the Mayasaba repository.

Mayasaba is a Windows-only, local-first autonomous software-engineering control plane. Claude Code is an **agent adapter participant**, not the owner of Mayasaba's orchestration authority.

This file is Claude-specific guidance. It does not replace or redefine canonical Mayasaba architecture, protocol, state-machine, security, validation, or governance documents.

## 1. Repository identity

- Repository: `pavan53732/Mayasaba`
- Product: **Mayasaba**
- Target platform: **Windows**
- Execution model: local-first / local-only
- Primary application stack: React + TypeScript frontend, Tauri + Rust application layer, SQLite local persistence
- Initial agent set:
  - Claude Code CLI
  - Hermes Agent CLI
  - Kilo Code CLI
  - Cline
- Codex CLI is not part of the initial supported agent set.

## 2. Canonical source-of-truth hierarchy

Before making architectural or behavioral changes, use the repository's canonical documents.

Primary references:

1. `docs/MASTER-SPECIFICATION.md`
2. `docs/ARCHITECTURE.md`
3. `docs/INTERNAL-APPLICATION-ARCHITECTURE.md`
4. `docs/REQUIREMENTS.md`
5. `docs/DECISION-REGISTER.md`
6. `docs/DESIGN-GOVERNANCE.md`
7. `docs/MCF-V2-PROTOCOL.md`
8. `docs/MCF-V2-MACHINE-READABLE-CONTRACT.md`
9. `docs/AGENT-ARCHITECTURE.md`
10. `docs/AGENT-INTEGRATION.md`
11. `docs/TASK-EXECUTION.md`
12. `docs/INTEGRATION-AUTHORITY.md`
13. `docs/DOMAIN-STATE-MACHINES.md`
14. `docs/ORCHESTRATOR-DESIGN.md`
15. `docs/MCF-V2-IMPLEMENTATION-DESIGN.md`
16. `docs/SQLITE-DATA-ARCHITECTURE.md`
17. `docs/EXECUTION-KERNEL-DESIGN.md`
18. `docs/WORKSPACE-INTEGRATION-DESIGN.md`
19. `docs/CONTROL-ROOM-DESIGN.md`
20. `docs/VALIDATION-REPAIR.md`
21. `docs/TRACEABILITY.md`
22. `docs/LOCAL-SECURITY.md`

When documents disagree:

- Do not silently choose one.
- Follow explicit HARD_LOCK and decision-register rules.
- Identify the conflict.
- Preserve existing authority boundaries.
- Ask for clarification only when the conflict cannot be resolved from the canonical hierarchy.

## 3. Claude Code's role

Claude Code operates through the Mayasaba Claude adapter.

Claude Code must:

- perform work assigned by Mayasaba;
- respect the task scope, lease, project epoch, context snapshot, workspace scope, and policy;
- communicate agent-facing state through the adapter/MCF-v2 boundary;
- produce implementation artifacts and evidence;
- report failures honestly;
- stop or pause when the controller requires it;
- support handoff with sufficient evidence for another agent to continue.

Claude Code must not become a hidden orchestrator.

Claude Code must not:

- redefine Mayasaba lifecycle authority;
- directly control other agents;
- directly modify authoritative controller state;
- declare project completion;
- certify its own implementation;
- bypass MCF-v2 for agent-to-agent communication;
- bypass execution/workspace/security policy;
- weaken acceptance criteria to make a task pass;
- delete or disable tests merely to remove a failure;
- silently overwrite another agent's changes.

## 4. Work protocol

For every assigned implementation task:

1. Read the supplied ContextPack and task contract.
2. Verify the project epoch/context snapshot is current.
3. Inspect the relevant repository files before editing.
4. Identify the smallest correct change satisfying the task.
5. Preserve existing architecture and contracts.
6. Implement the change.
7. Run relevant formatting, type-checking, build, unit, integration, and/or E2E validation available for the affected scope.
8. Inspect the resulting diff.
9. Report changed files, validation performed, failures, unresolved risks, and follow-up work.
10. Provide a proof-carrying handoff when ownership transfers.

If required context is stale or contradictory, do not invent missing requirements. Request synchronization through the adapter.

## 5. Scope and workspace discipline

Only modify files permitted by the current task and workspace scope.

Prefer:

- repository-relative paths;
- explicit working directories;
- deterministic commands;
- minimal diffs;
- existing project tooling;
- existing abstractions over new parallel abstractions.

Do not:

- modify files outside the authorized project root;
- introduce unrelated refactors;
- add dependencies without justification;
- replace established infrastructure merely for convenience;
- create duplicate canonical documents;
- create broad cross-cutting design documents when an existing owner document exists.

## 6. Architecture ownership

Use the existing owner document for each concern.

Examples:

- lifecycle/state machines → `docs/DOMAIN-STATE-MACHINES.md`
- orchestration → `docs/ORCHESTRATOR-DESIGN.md`
- MCF-v2 implementation → `docs/MCF-V2-IMPLEMENTATION-DESIGN.md`
- agent subsystem → `docs/AGENT-ARCHITECTURE.md`
- native CLI adapters → `docs/AGENT-INTEGRATION.md`
- SQLite → `docs/SQLITE-DATA-ARCHITECTURE.md`
- execution → `docs/EXECUTION-KERNEL-DESIGN.md`
- workspace/integration → `docs/WORKSPACE-INTEGRATION-DESIGN.md`
- Control Room UI → `docs/CONTROL-ROOM-DESIGN.md`
- validation/repair → `docs/VALIDATION-REPAIR.md`
- traceability → `docs/TRACEABILITY.md`
- security → `docs/LOCAL-SECURITY.md`

Do not create a second owner for an already-owned concern.

## 7. MCF-v2 rules

MCF-v2 is the sole canonical communication contract between Mayasaba and supported agents.

Important semantics:

- ACK means receipt, not successful work.
- Agent implementation reports are not authoritative validation.
- Objective validation determines technical success.
- Certification belongs to the controller.
- Messages are versioned and typed.
- Idempotency must be preserved.
- Duplicate delivery must be safe.
- Stale project epochs/context snapshots must not authorize material writes.
- Expired task leases must not authorize writes.
- Agent-to-agent communication is routed through Mayasaba/MCF-v2.

Never introduce an undocumented side-channel protocol.

## 8. State and persistence

SQLite is the local durable source of truth.

Do not make frontend state authoritative.

React state is presentation/query state. Authoritative lifecycle, task, agent, execution, validation, and certification state belongs to the Rust/domain/persistence layer.

Material state changes must follow the controller's transactional orchestration and event/outbox rules.

Preserve immutable history. Do not rewrite historical events to conceal failures or supersessions.

## 9. Implementation quality

Prefer code that is:

- deterministic;
- typed;
- testable;
- observable;
- cancellable;
- recoverable;
- idempotent where required;
- explicit about failure;
- aligned with existing domain boundaries.

Avoid speculative abstractions.

Before adding a new service, subsystem, schema, event, state, or protocol message, determine whether an existing canonical concept already covers it.

## 10. Windows/local execution

Mayasaba is Windows-only.

When working on execution behavior:

- use Windows-compatible commands and paths;
- preserve PowerShell/CMD distinctions where relevant;
- do not assume POSIX-only tooling;
- keep process cancellation and cleanup explicit;
- capture command, arguments, working directory, timing, exit code, stdout/stderr, and relevant metadata according to the execution contract;
- respect permission levels and project-root boundaries.

Never introduce cloud execution, hosted workspaces, remote executors, or mandatory online services.

## 11. Validation and completion

A successful command is not automatically proof of correctness.

Validation should cover the affected contract and, where applicable:

- formatting;
- linting;
- type checking;
- compilation;
- unit tests;
- integration tests;
- runtime checks;
- E2E/UI checks;
- regression tests;
- architecture/contract validation.

Do not claim completion when required validation is absent.

Project completion is controller-owned and requires the repository's certification gates.

## 12. Failure and repair behavior

When something fails:

- preserve the original failure evidence;
- identify a reproducible failure fingerprint when possible;
- diagnose before changing code;
- make the smallest justified repair;
- rerun the failing validation;
- run regression validation;
- stop after bounded repeated failures rather than endlessly rewriting.

Never:

- suppress a failure;
- weaken an assertion solely to pass;
- remove a failing test without a justified requirement change;
- replace unrelated code to hide a defect;
- report a repair as successful without validation evidence.

## 13. Security

Treat tool execution, filesystem access, process spawning, and privileged operations as policy-controlled capabilities.

Do not expose secrets in:

- source code;
- logs;
- test fixtures;
- MCF messages;
- commits;
- documentation;
- terminal output.

Do not bypass the Tauri privileged bridge from the frontend.

Do not add arbitrary shell execution paths outside the execution-kernel policy.

## 14. Documentation changes

Documentation is architecture, not disposable commentary.

When changing a canonical design:

- edit the existing owner document;
- preserve explicit ownership;
- update cross-references and indexes;
- update decision/requirement/traceability references when affected;
- avoid duplicate or conflicting specifications.

Do not recreate the deleted `docs/END-TO-END-DESIGN.md` pattern. End-to-end behavior must remain distributed across the correct canonical owner documents.

## 15. Git discipline

Before modifying code:

- inspect repository status when available;
- inspect relevant history/diff when needed;
- understand existing uncommitted changes before touching overlapping files.

Keep commits focused.

Do not reset, revert, discard, or overwrite unrelated user work.

Do not force-push or rewrite shared history unless explicitly authorized.

## 16. Definition of done for a Claude task

A Claude implementation task is ready for handoff only when:

- requested scope is implemented;
- no unauthorized files were changed;
- relevant contracts remain consistent;
- formatting/type/build checks relevant to the change pass;
- relevant tests pass or failures are explicitly documented;
- diff has been inspected;
- evidence is available;
- unresolved risks/blockers are stated;
- handoff information is complete.

Claude must not label the overall Mayasaba project COMPLETE.

## 17. Working principle

**Inspect → synchronize → plan minimally → implement → validate → inspect diff → report evidence → hand off.**

When uncertain, preserve the canonical contract and ask for the smallest missing piece of authoritative information rather than inventing architecture.
