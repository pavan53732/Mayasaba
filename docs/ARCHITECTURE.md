# Mayasaba Master Architecture

## 1. System role

Mayasaba is the deterministic local control plane for autonomous software engineering using four independent coding CLIs.

It coordinates, but does not replace, the intelligence of:

- Claude Code CLI
- Hermes Agent CLI
- Kilo Code CLI
- Cline

## 2. Architectural principle

**One project reality, four independent intelligences.**

Agents do not share an implicit brain. Mayasaba maintains authoritative project facts, requirements, decisions, task ownership, context versions, workspace scope, execution evidence and validation state.

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
          ┌─────────────┼─────────────┬─────────────┐
          ▼             ▼             ▼             ▼
       Claude         Hermes         Kilo          Cline
       Adapter        Adapter        Adapter       Adapter
          │             │             │             │
          └─────────────┴─────────────┴─────────────┘
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
Project-root scope, Git worktrees, checkpoints, integration workspace and non-Git isolation.

### crates/execution
PowerShell/CMD/process execution, timeouts, cancellation, child-process cleanup and command evidence.

### crates/validation
Build/test/E2E/review gates and deterministic pass/fail rules.

### crates/evidence
Content-addressed artifacts and evidence bundles.

### crates/policy
Permissions, destructive-operation controls, install/admin escalation and secret handling.

### crates/storage
SQLite schema, transactions, state persistence and event history.

## 5. State architecture

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

## 6. Material action gates

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

## 7. Workspace model

Git worktrees/branches are preferred for concurrent agents.

The integration workspace is controller-owned. Agents work in isolated scopes and submit changes/evidence for integration.

Non-Git projects use scoped filesystem isolation and checkpoints.

## 8. Recovery

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

## 9. Completion

An agent can never certify the project. The controller may enter COMPLETE only after evidence-backed validation and packaging gates pass.


The detailed orchestration contract is defined in `ORCHESTRATOR-DESIGN.md` and authoritative internal transitions in `DOMAIN-STATE-MACHINES.md`.

MCF-v2 implementation mechanics are defined in `MCF-V2-IMPLEMENTATION-DESIGN.md`.

SQLite implementation structure is defined in `SQLITE-DATA-ARCHITECTURE.md`.