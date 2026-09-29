# Mayasaba Design Governance

## Canonical rule

The GitHub repository `pavan53732/Mayasaba` is the single canonical implementation repository for Mayasaba.

## Change classes

Every architecture/protocol change must be classified:

- ADDITIVE — introduces a new compatible capability
- REFINEMENT — improves an existing definition without changing its fundamental ownership
- REPLACEMENT — intentionally supersedes an existing design
- DEPRECATION — retires an existing capability

## Collision prevention

Do not silently:

- redefine existing terms
- create another canonical message envelope
- create another message registry
- create another state source of truth
- create another task lease system
- create another communication bus
- create another context authority
- create another handoff contract

Reference and refine canonical definitions instead.

## Ownership

Protocol → crates/protocol

Bus → crates/bus

Agents → crates/agents

Orchestrator → crates/core

Tasks/leases → crates/tasks

Council → crates/council

Execution → crates/execution

Workspace → crates/workspace

Validation → crates/validation

Evidence → crates/evidence

Policy → crates/policy

Persistence → crates/storage

## Required change record

Any meaningful change must document:

1. affected subsystem
2. previous behavior
3. new behavior
4. compatibility impact
5. migration/reconciliation
6. tests required
7. classification

## Authority order

1. current user-approved requirements
2. HARD_LOCK decisions
3. versioned architecture/contracts
4. verified workspace facts
5. objective evidence
6. persisted orchestration state
7. agent proposals/reports

## Historical integrity

Do not erase historical decisions/events to make the current state look cleaner. Supersede them explicitly and preserve traceability.


## Machine-readable ownership registries

- MCF-v2 message/event/priority registry: `schemas/mcf-v2/registry.json`.
- Tauri bridge command/query/event registry: `schemas/tauri-bridge-v1/bridge.schema.json`.
- Agent runtime probe result: `schemas/agent-adapter-v1/probe-result.schema.json`.
- Workspace/crate dependency contract: `docs/WORKSPACE-MANIFEST.md`.

These are source-of-truth artifacts, not additional competing architecture documents.