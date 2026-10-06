# Mayasaba Workspace Manifest

## Authority

This is the human-readable workspace/dependency ownership contract. The machine-readable source is `workspace.manifest.json`; this document explains its intent and implementation alignment.

## Target repository layout

```
Mayasaba/
  AGENTS.md
  README.md
  Cargo.toml
  package.json
  apps/
    desktop/
      package.json
      vite.config.ts
      src/
      src-tauri/
        Cargo.toml
        tauri.conf.json
        src/
  crates/
    core/
    protocol/
    bus/
    agents/
    council/
    tasks/
    workspace/
    execution/
    validation/
    evidence/
    policy/
    storage/
  schemas/
    mcf-v2/
    tauri-bridge-v1/
```

## Crate ownership

| Crate | Owns | Depends on |
|---|---|---|
| core | lifecycle, orchestration, application services, cross-subsystem gates | protocol, bus, agents, council, tasks, workspace, execution, validation, evidence, policy, storage |
| protocol | MCF-v2 types, registry, schema validation | no domain crates |
| bus | durable messaging, routing, outbox/inbox | protocol, storage |
| agents | agent registry, sessions, adapters | protocol, execution, workspace |
| council | rounds, debate, barriers, decisions | protocol, storage |
| tasks | requirements-to-task graph, leases, handoffs | protocol, storage, agents, workspace |
| workspace | filesystem scope, Git worktrees, checkpoints, integration | storage, policy |
| execution | PowerShell/CMD/process lifecycle | storage, policy |
| validation | task-appropriate integrity/review gates; software build/test/E2E when applicable | execution, workspace, evidence, storage |
| evidence | artifacts/evidence provenance | storage |
| policy | permission and security decisions | storage |
| storage | SQLite schema, migrations, transactions | OS/filesystem primitives only |

### Context ownership

Context is a domain service within `crates/core` for cross-cutting orchestration and persistence semantics. There is intentionally no separate `crates/context` in the 12-crate target graph.

## Application shell

`apps/desktop/src-tauri` owns Tauri commands, generated bridge DTOs and process startup only. It calls `crates/core`; it does not become a second domain authority.

`apps/desktop/src` owns React presentation, generated bridge types and derived UI stores only.

## Dependency rules

- No crate may depend upward on the desktop UI.
- `protocol` depends on no agent/domain implementation.
- `storage` depends on no higher-level crate.
- `agents` never depends on another agent adapter.
- `core` is the only cross-subsystem orchestration owner.
- Domain crates publish typed events through core/bus boundaries; they do not create ad-hoc callbacks.
- Tauri is an application transport, not a domain layer.

## Manifest alignment

When implementation starts, the following files must encode this graph:

- root `Cargo.toml`: Rust workspace members.
- each `crates/*/Cargo.toml`: direct dependencies only.
- `apps/desktop/src-tauri/Cargo.toml`: Tauri shell + `mayasaba-core` and required generated bridge support.
- root `package.json`: workspace/tooling scripts.
- `apps/desktop/package.json`: React/Tauri frontend dependencies.
- `apps/desktop/src-tauri/tauri.conf.json`: Windows/MSI packaging and resource declarations.

No dependency may exist only in prose after implementation begins; the local contract gate (`npm run verify:contracts`) must compare manifests with this matrix. As implemented today the gate checks that each declared manifest exists and that every crate's `Cargo.toml` names its declared `mayasaba-*` dependencies; a full content comparison of the root `Cargo.toml`, desktop `package.json` and `tauri.conf.json` against this matrix is not yet implemented.


## Application-service ownership

The complete service → crate mapping is authoritative in `workspace.manifest.json`. It includes all 22 application services and prevents the application-service vocabulary from becoming a second crate ownership system.

Key mappings:

- ProjectService, LifecycleService, RequirementService, DecisionService, ArchitectureService, AttachmentService, ContextService, RepairService, RecoveryService, ConfigurationService, SimulationService, DiagnosticsService → `crates/core`
- AgentService → `crates/agents`
- CouncilService → `crates/council`
- TaskService → `crates/tasks`
- WorkspaceService → `crates/workspace`
- ExecutionService → `crates/execution`
- BuildService, TestService, ValidationService, ReviewService → `crates/validation`
- EvidenceService → `crates/evidence`
- PolicyService → `crates/policy`

There is no one-crate-per-service requirement.

## Tauri bridge ownership and code generation

`schemas/tauri-bridge-v1/bridge.schema.json` is the canonical identifier schema for commands, queries, channels and UI event types. `workspace.manifest.json` is the canonical command/query/event → service mapping.

The implementation path is:

```text
bridge.schema.json + workspace.manifest.json
        ↓
deterministic code generation
        ↓
Rust bridge DTO/metadata + TypeScript bridge types
        ↓
Tauri command/event runtime
```

Generated outputs are checked in and must not be hand-edited. `tools/codegen/generate-bridge.mjs` emits both surfaces — `apps/desktop/src/generated/bridge.ts` and `apps/desktop/src-tauri/src/generated/bridge.rs` — from the identifier schema, and its `--check` mode compares both against what the contract implies, so the "not hand-edited" rule is mechanically enforced. The local contract gate (`npm run verify:contracts`) runs that check and additionally reads both sides of the bridge: the registered handlers in `apps/desktop/src-tauri/src/main.rs` and the frontend's `transport(...)` call sites. A handler or a call naming an operation the contract does not declare fails; a declared operation that nothing implements is reported with its count (DEC-053).

## Runtime schema distribution

MCF-v2, Tauri bridge, transition and agent-probe schemas are repository build inputs and are embedded into the installed Mayasaba binary/runtime. The MSI must not depend on repository-relative schema paths.

The embedding owner is `crates/protocol`; compile-time embedding such as Rust `include_str!`/`include_bytes!` (or an equivalent deterministic build step) is authoritative.

## Implementation-manifest gate

Before M0.5 exits, the repository must contain the root `Cargo.toml`, desktop `package.json`, Tauri configuration and crate manifests matching `workspace.manifest.json`. The local contract gate (`npm run verify:contracts`) currently checks manifest existence and that every crate's `Cargo.toml` names its declared `mayasaba-*` dependencies; a full content comparison is planned but not yet implemented (see DEC-036).
