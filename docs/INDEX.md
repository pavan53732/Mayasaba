# Mayasaba Documentation Index

This directory is the canonical product, architecture, protocol, implementation, and governance documentation for Mayasaba.

## Start here

- [Master Specification](MASTER-SPECIFICATION.md) — single integrated product/system contract
- [Architecture](ARCHITECTURE.md) — runtime/component architecture
- [Internal Application Architecture](INTERNAL-APPLICATION-ARCHITECTURE.md) — frontend, Tauri bridge, Rust services, domain, orchestration, configuration, recovery and simulation
- [Requirements](REQUIREMENTS.md) — functional and non-functional requirements
- [MCF-v2 Protocol](MCF-V2-PROTOCOL.md) — communication semantics
- [MCF-v2 Machine-Readable Contract](MCF-V2-MACHINE-READABLE-CONTRACT.md) — schema package contract
- [MCF-v2 Registry](MCF-V2-REGISTRY.md) — canonical message/event/priority registry
- [Workspace Manifest](WORKSPACE-MANIFEST.md) — canonical crate/dependency/build graph
- [Agent Architecture](AGENT-ARCHITECTURE.md) — agent registry, sessions, capabilities, health, routing and recovery
- [Agent Integration](AGENT-INTEGRATION.md) — native CLI facts, probes and adapter contract
- [AGENTS.md](../AGENTS.md) — mandatory repository operating instructions for AI coding agents
- [Council Engine](COUNCIL-ENGINE.md) — multi-agent deliberation and user interview model
- [Task & Execution](TASK-EXECUTION.md) — task DAG, leases, workspaces and local execution
- [Control Room](CONTROL-ROOM.md) — desktop UX and observability
- [Validation & Repair](VALIDATION-REPAIR.md) — testing, diagnosis, repair, regression and certification
- [Data Model](DATA-MODEL.md) — durable entities and SQLite responsibilities
- [Memory & Context](MEMORY-CONTEXT.md) — project memory, epochs, snapshots and context synchronization
- [Bootstrap / Doctor](BOOTSTRAP-DOCTOR.md) — Windows preflight and runtime agent detection
- [Conformance & Testing](CONFORMANCE-AND-TESTING.md) — protocol, adapter, recovery and autonomy validation
- [Traceability](TRACEABILITY.md) — requirement-to-evidence coverage
- [Integration Authority](INTEGRATION-AUTHORITY.md) — merge, checkpoint and authoritative workspace control
- [Local Security](LOCAL-SECURITY.md) — permissions, secrets, isolation and recovery
- [Design Governance](DESIGN-GOVERNANCE.md) — rules preventing architecture/document collisions
- [Design History](DESIGN-HISTORY.md) — consolidated record of architectural decisions made so far
- [Decision Register](DECISION-REGISTER.md) — current locked decisions
- [Roadmap](ROADMAP.md) — implementation sequence

## Canonical ownership

The authoritative crate/dependency ownership matrix is maintained only in [WORKSPACE-MANIFEST.md](WORKSPACE-MANIFEST.md). This index intentionally does not duplicate the table.

## Documentation rule

When information belongs to an existing canonical document, update that document or link to it. Do not create duplicate specifications for the same responsibility.

- [Domain State Machines](DOMAIN-STATE-MACHINES.md) — authoritative internal lifecycle and transition ownership
- [Orchestrator Design](ORCHESTRATOR-DESIGN.md) — deterministic scheduling, gates, recovery and lifecycle coordination
- [MCF-v2 Implementation Design](MCF-V2-IMPLEMENTATION-DESIGN.md) — durable bus implementation mechanics
- [SQLite Data Architecture](SQLITE-DATA-ARCHITECTURE.md) — relational persistence and transaction structure
- [Execution Kernel Design](EXECUTION-KERNEL-DESIGN.md) — local Windows process execution
- [Workspace & Integration Design](WORKSPACE-INTEGRATION-DESIGN.md) — worktrees, integration, checkpoints and rollback
- [Control Room UX Architecture](CONTROL-ROOM-DESIGN.md) — UI composition and authoritative-state presentation