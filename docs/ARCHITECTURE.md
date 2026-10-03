# Mayasaba Master Architecture

## 1. System role

Mayasaba is the deterministic local control plane for user-authorized work on local files, including software engineering and document, research-report, and data tasks, using three coding CLIs.

It coordinates, but does not replace, the intelligence of:

- Hermes Agent CLI
- Kilo Code CLI
- OpenCode CLI

## 2. Architectural principle

**One project reality, three separate agent sessions.**

Agents do not share an implicit brain. Mayasaba maintains authoritative project facts, requirements, decisions, task ownership, context versions, workspace scope, execution evidence and validation state.

**Independence caveat.** The three sessions are architecturally separate — each holds its own model/provider choice, authentication, session and reasoning — but they are not three independent implementations. Kilo Code CLI is a fork of OpenCode CLI and shares its codebase lineage, config surface and much of its permission model. Do not treat Kilo and OpenCode as independent corroboration of each other in council deliberation: agreement between them is weaker evidence than agreement between either and Hermes. Model/provider diversity remains real and is the property the council actually relies on.

All agent communication is brokered through MCF-v2.

## 3. Layered architecture

~~~text
┌──────────────────────────────────────────────────────┐
│                  CONTROL ROOM                        │
│ Chat • Council • Tasks • Builds • Tests • Evidence   │
└───────────────────────┬──────────────────────────────┘
                        │
┌───────────────────────▼──────────────────────────────┐
│                ORCHESTRATOR / CORE                  │
│ phase transitions • scheduling • barriers • gates    │
└───────────────────────┬──────────────────────────────┘
                        │
┌───────────────────────▼──────────────────────────────┐
│                    MCF-v2 BUS                       │
│ protocol • routing • ACK • retry • dedupe • replay  │
└───────────────────────┬──────────────────────────────┘
                        │
          ┌─────────────┼─────────────┐
          ▼             ▼             ▼
        Hermes         Kilo        OpenCode
        Adapter        Adapter       Adapter
           │             │             │
           └─────────────┴─────────────┘
                        │
             ┌──────────▼──────────┐
             │ Workspace Manager   │
             │ Execution Kernel    │
             └──────────┬──────────┘
                        │
             Build / Test / E2E / Review
                        │
             Validation / Repair Engine
                        │
                    Evidence Store
                        │
                      SQLite
~~~

## 4. Core modules

### crates/core
Project lifecycle authority, orchestration, phase transitions and aggregate state.

### crates/protocol
Canonical MCF-v2 types, JSON Schema compatibility and protocol validation.

### crates/bus
Durable local routing, queueing, priorities, ACK/NACK, idempotency, retries, ordering, dead-letter, outbox/inbox and replay.

### crates/agents
Runtime detection, adapter sessions, capability negotiation and native-protocol translation.

### crates/council
Independent analysis, debate, questions, barriers, disagreement resolution and decision locks.

### crates/tasks
Requirement-to-task transformation, DAG scheduling, task ownership and leases.

### crates/workspace
User-selected workspace/task-path scope, Git worktrees, checkpoints, integration workspace and non-Git isolation.

### crates/execution
PowerShell/CMD/process execution, timeouts, cancellation, child-process cleanup and command evidence.

### crates/validation
Task-appropriate deterministic checks: software build/test/E2E/review gates when applicable, plus document, research and data-artifact integrity checks.

### crates/evidence
Content-addressed artifacts and evidence bundles.

### crates/policy
Permissions, destructive-operation controls, install/admin escalation and secret handling.

### crates/storage
SQLite schema, transactions, state persistence and event history.

## 5. Workspace and dependency authority

The 12-crate dependency graph, application-shell placement and manifest alignment are defined only in `WORKSPACE-MANIFEST.md`. That document is the dependency source of truth; this file remains the runtime architecture overview.

## 6. State architecture

Do not implement a monolithic state enum. The following state machines are independent and authoritative within their ownership boundary:

- project/orchestrator
- agent session
- message delivery
- context
- task/lease
- council round/barrier
- handoff
- execution
- validation

The orchestrator derives a project-level status from these states.

## 7. Material action gates

Before a material state-changing action:

~~~text
Identity
  + Session
  + Project
  + Workspace
  + Capability
  + Policy
  + Task Lease
  + Current Context/Epoch
  + Relevant Requirement/Decision/Contract
  ↓
AUTHORIZED ACTION
~~~

A failed gate produces an explicit blocker/error and no unauthorized action.

## 8. Workspace model

A workspace is a user-selected local folder and task-scoped set of allowed paths. It may contain code or other local artifacts such as documents, reports and datasets; Mayasaba does not scan the entire PC by default. Out-of-scope file access or writes require explicit scope approval.

Git worktrees/branches are preferred for concurrent agents on software repositories. The integration workspace is controller-owned. Agents work in isolated scopes and submit changes/evidence for integration.

Non-Git and file-focused workspaces use scoped filesystem isolation and checkpoints.

## 9. Recovery

Mayasaba survives:

- UI restart
- controller restart
- agent crash
- adapter restart
- duplicate message
- delayed delivery
- stale context
- lease expiry
- partial council participation

Recovery uses durable events, outbox/inbox reconciliation, process verification, workspace verification, lease reconciliation and context rehydration.

## 10. Completion

| agent can never certify the work. The controller may enter COMPLETE only after the task's applicable acceptance and validation gates pass with evidence. Build, runtime, E2E and packaging gates apply to software work when required; they are not universal gates for document, research or data tasks. Integration is reachable only through a persisted INTEGRATION_ADMISSION record with verdict ADMITTED.

Mayasaba supports user-requested read-only retrieval from public web sources for local research reports. It does not provide email, posting, form-submission, purchase, account-change or general desktop-control actions.


The detailed orchestration contract is defined in `ORCHESTRATOR-DESIGN.md` and authoritative internal transitions in `DOMAIN-STATE-MACHINES.md`.

MCF-v2 implementation mechanics are defined in `MCF-V2-IMPLEMENTATION-DESIGN.md`.

SQLite implementation structure is defined in `SQLITE-DATA-ARCHITECTURE.md`.