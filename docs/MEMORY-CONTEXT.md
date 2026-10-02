# Mayasaba Memory and Context Architecture

## Purpose

Mayasaba must preserve project continuity without turning independent agents into one shared AI mind.

## Authority model

Mayasaba stores authoritative project facts and produces task-specific ContextPacks.

Agents retain independent model context. Mayasaba provides synchronized project truth.

## Memory layers

### 1. Project state
Durable requirements, decisions, architecture, contracts, tasks, workspace state, validation state and evidence references.

### 2. Session state
Current agent sessions, council rounds, active tasks, leases, pending handoffs and communication state.

### 3. Context snapshot
Immutable task/council context generated from current authoritative project state.

### 4. Context delta
Minimal change set from a known snapshot to the latest valid snapshot.

### 5. Event history
Immutable causal history used for audit, replay and recovery.

## Decision locks

Decision classes:

- HARD_LOCK
- SOFT_DECISION
- ASSUMPTION
- OPEN

HARD_LOCK changes require explicit reopening and impact analysis.

## Project epoch

project_epoch increments when material project truth changes.

Examples:

- requirement change
- HARD_LOCK change
- architecture/contract change
- task graph semantic change
- security/policy change

Affected contexts become stale.

## Context freshness

Every material agent operation references:

- project epoch
- context snapshot
- state digest
- relevant requirement hash
- relevant decision hashes
- relevant contract hashes

An agent operating from a stale context is blocked from material state-changing work until rehydrated.

## Context Pack contents

A task-specific ContextPack can include:

- objective
- requirements
- acceptance criteria
- decisions
- architecture/contract references
- task dependencies
- workspace scope
- relevant files
- recent failures
- recent repairs
- validation requirements
- policy constraints
- evidence references
- state digest

Only relevant project context should be provided.

### Architecture-task ContextPacks

For a runtime task that proposes or changes Mayasaba architecture, contracts, schemas or canonical architecture documentation, the ContextPack MUST carry:

- applicable product-owned runtime policy and the operative, task-scoped architecture co-design instruction in `policy_constraints` (include the instruction, not just a path the agent may be unable to discover);
- relevant accepted decisions and HARD_LOCKs, plus unresolved questions or proposals clearly labeled as such, in `decisions`;
- canonical owner-document and schema references in `architecture_refs`, and relevant interface/data-contract references in `contracts`.

The co-design instruction is part of this runtime ContextPack contract; it is not sourced from repository contributor files. It directs the agent to establish the current contract, identify the concrete gap, recommend a normative resolution and its impacts, distinguish accepted decisions from proposals, and honor the discussion-versus-implementation boundary. Keep the pack task-scoped and preserve source references so agents can trace each rule and decision to its owner.

The Mayasaba repository's root `AGENTS.md` is guidance for coding agents working in that repository, and `CLAUDE.md` is a Claude Code-specific overlay. They are not universal runtime-policy sources. If the selected task workspace is the Mayasaba repository, the applicable repository instructions may be included as task-scoped workspace guidance, clearly distinguished from controller-owned policy; `CLAUDE.md` applies only to Claude Code.

Use only existing ContextPack fields. This requirement does not change the MCF-v2 envelope or payload schema; in particular, it adds no ContextPack fields.

### Council answer propagation

When a user answers a Council question, CouncilService first persists the full immutable `UserAnswer`—including its source question and effective redistribution scope, not just free-text—and routes any accepted requirement/decision change through its owning service. If the submitted scope is omitted, resolve it from the question's affected agents before persisting. ContextService then rebuilds the affected ContextPacks from authoritative state; the answer is not treated as an authoritative project fact merely because it appeared in chat.

- If the answer changes material requirements, decisions, contracts, scope or other project truth, update the owning record and increment `project_epoch` under the existing epoch rules.
- If the answer changes an agent's applicable context without changing project truth, create a new snapshot and digest at the current epoch. In either case, supersede or invalidate prior snapshots for affected work so stale context cannot authorize writes.
- Use the answer's `redistribution_scope` when provided, restricted to affected agents in the same project/round. Otherwise use the question's `affected_agents`; if that set was not established, CouncilService must resolve the recipients from the active round before asking the user.
- Distribute the new snapshot through the existing MCF-v2 `CONTEXT_UPDATE` mechanism, which carries the snapshot ID, epoch and digest. Send `DECISION` only if a formal decision was persisted. Do not introduce a new MCF-v2 message type or payload field for user answers.
- Resume the council with a linked revision/continuation round. A receipt ACK alone does not prove an agent has rehydrated; material work remains gated on current context and lease validation.

## Shared truth is not shared reasoning

Two agents may disagree even when they received the same ContextPack. Mayasaba preserves both positions and resolves the issue through evidence, council deliberation or user input.

## Historical integrity

Old messages and snapshots remain historical records. They are never rewritten to make current state appear consistent.

A newer decision supersedes an older one through an explicit version/supersession relationship.

## Recovery

After restart, Mayasaba regenerates valid current context from durable state instead of trusting an agent's private session memory.


## Canonical state-digest algorithm

The state digest is a deterministic SHA-256 hash over the RFC 8785 JSON Canonicalization Scheme (JCS) serialization of the authoritative scoped input.

Digest input version: `1.0`.

The input contains exactly the normalized fields declared by `schemas/context-v1/context-digest.schema.json`:

- requirements
- decisions
- architecture
- contracts
- task/dependencies
- workspace scope/checkpoint identity
- validation requirements/results
- policy constraints

Rules:

1. Object keys are canonicalized with RFC 8785 JCS.
2. Arrays representing sets are sorted by stable identifier before canonicalization.
3. Semantically ordered arrays retain their declared order.
4. Volatile timestamps, logs, display state and agent prose are excluded unless explicitly part of authoritative task/validation state.
5. The scope and project epoch are included in the canonical input.
6. SHA-256 is computed over the UTF-8 bytes of the canonical JSON.
7. Any authoritative change to an input field produces a different digest.
8. A receiver compares the envelope digest against the digest recomputed from current authoritative state for the same project/epoch/scope. A mismatch is a stale-context condition.

Digest fixtures must cover identical-input equality, key-order independence, set-order normalization, changed requirement, changed lease/workspace and changed epoch.
