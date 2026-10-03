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
- material project-intent (ProjectBrief) change

Affected contexts become stale.

## User contributions and the epoch

A free-text user message submitted after project creation is recorded as a `UserContribution` with an **advisory** classification. The classification is an input to routing, not an authority: only the owning service that would mutate the affected record determines whether material project truth actually changed. The advisory classification is produced by the intake router (ConfigurationService policy), is stored on the `UserContribution` row, and is never read as authorization by the owning service.

| Outcome | Epoch | Context |
|---|---|---|
| Material truth change (for example a new requirement, a goal change or a constraint change) | increments | affected contexts stale |
| Non-material change to an agent's applicable knowledge (for example a clarification) | unchanged | new snapshot and digest at the current epoch |
| No change (commentary) | unchanged | timeline only |

A mislabeled contribution cannot cause or avoid an epoch transition. A message classified as a clarification that in fact reveals a material constraint increments the epoch when the owning service accepts the resulting truth change; a message classified as a requirement that the owning service finds already satisfied does not.

Free-text contributions never bypass the existing mediated commands. `answer_user_question`, `reopen_decision`, `approve_action` and the lifecycle controls remain separate, gated paths; a contribution is not an alternative route into CouncilService, DecisionService or LifecycleService.

## Project brief as analysis anchor

`ProjectBrief` versions are immutable. The brief version current when DISCOVERY closes is the analysis anchor referenced by the resulting context snapshots and council rounds for that lineage. A later brief version does not rewrite the historical meaning of a snapshot or council round that consumed an earlier one; it takes effect through the ordinary change-impact path (epoch and snapshot rules above).

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

The Mayasaba repository's root `AGENTS.md` is guidance for coding agents working in that repository. It is the single canonical repository instruction file and is not a universal runtime-policy source. If the selected task workspace is the Mayasaba repository, the applicable repository instructions may be included as task-scoped workspace guidance, clearly distinguished from controller-owned policy.

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

## Canonical event hash chain

The state digest protects context synchronization. A separate chain protects the durable event history itself.

Every persisted event carries `prev_hash` and `event_hash`, forming a per-project SHA-256 hash chain. It uses the same canonicalization rule as the state digest: SHA-256 over the RFC 8785 JCS serialization of the event's authoritative fields.

1. Object keys are canonicalized with RFC 8785 JCS.
2. The input is exactly `{prev_hash, event_id, project_id, session_id, event_type, sequence, correlation_id, causation_id, epoch, payload_json, created_at}`.
3. The first event of a project chain uses a genesis `prev_hash` of 64 zero characters.
4. Each subsequent `prev_hash` equals the preceding event's `event_hash`, ordered by `sequence`.
5. SHA-256 is computed over the UTF-8 bytes of the canonical JSON.
6. Verification recomputes the chain from persisted rows and fails at the first mismatch, naming the divergent `event_id` and `sequence`.

The chain and the state digest are complementary and must not be conflated. The state digest answers "is this agent's context stale?" and covers a scoped projection of current authoritative state. The event chain answers "has durable history been altered?" and covers every persisted event in order. An epoch change alters many digests while leaving the chain intact, because the chain records that events were appended, not what they mean.

Chain fixtures must cover: a valid chain verifies; a mutated field breaks verification at that event and every later event; a deleted event breaks the chain at its successor; a reordered pair breaks the chain; two projects chain independently; and events with a null project_id chain by session.
