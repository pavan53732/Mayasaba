# Mayasaba Documentation Index

This directory is the canonical product, architecture, protocol, implementation, and governance documentation for Mayasaba.

## Start here

- [Master Specification](MASTER-SPECIFICATION.md) — single integrated product/system contract
- [Architecture](ARCHITECTURE.md) — runtime/component architecture
- [Internal Application Architecture](INTERNAL-APPLICATION-ARCHITECTURE.md) — frontend, Tauri bridge, Rust services, domain, orchestration, configuration, recovery and simulation
- [Requirements](REQUIREMENTS.md) — functional and non-functional requirements
- [MCF-v2 Protocol](MCF-V2-PROTOCOL.md) — communication semantics
- [MCF-v2 Machine-Readable Contract](MCF-V2-MACHINE-READABLE-CONTRACT.md) — schema package contract
- [Agent Architecture](AGENT-ARCHITECTURE.md) — agent registry, sessions, capabilities, health, routing and recovery
- [Agent Integration](AGENT-INTEGRATION.md) — four CLI adapter architecture
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

| Responsibility | Owner |
|---|---|
| Wire protocol | crates/protocol |
| Delivery/bus | crates/bus |
| Agent adapters | crates/agents |
| Orchestration | crates/core |
| Council | crates/council |
| Tasks/leases | crates/tasks |
| Execution | crates/execution |
| Workspace | crates/workspace |
| Validation | crates/validation |
| Evidence | crates/evidence |
| Policy | crates/policy |
| Persistence | crates/storage |

## Documentation rule

When information belongs to an existing canonical document, update that document or link to it. Do not create duplicate specifications for the same responsibility.
