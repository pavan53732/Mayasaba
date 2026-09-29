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

## Shared truth is not shared reasoning

Two agents may disagree even when they received the same ContextPack. Mayasaba preserves both positions and resolves the issue through evidence, council deliberation or user input.

## Historical integrity

Old messages and snapshots remain historical records. They are never rewritten to make current state appear consistent.

A newer decision supersedes an older one through an explicit version/supersession relationship.

## Recovery

After restart, Mayasaba regenerates valid current context from durable state instead of trusting an agent's private session memory.
