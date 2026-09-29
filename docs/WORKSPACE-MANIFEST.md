# Mayasaba Workspace Manifest

## Authority

This is the implementation-level workspace/dependency ownership contract. It defines the target repository graph; it does not replace Cargo/npm/Tauri manifests once implementation begins.

## Target repository layout

```
Mayasaba/
  AGENTS.md
  CLAUDE.md
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
| core | lifecycle, orchestration, application services, cross-subsystem gates | protocol, bus, agents, council, context, tasks, workspace, execution, validation, evidence, policy, storage |
| protocol | MCF-v2 types, registry, schema validation | no domain crates |
| bus | durable messaging, routing, outbox/inbox | protocol, storage |
| agents | agent registry, sessions, adapters | protocol, execution, workspace |
| council | rounds, debate, barriers, decisions | protocol, storage, context |
| tasks | requirements-to-task graph, leases, handoffs | protocol, storage, agents, workspace |
| workspace | filesystem scope, Git worktrees, checkpoints, integration | storage, policy |
| execution | PowerShell/CMD/process lifecycle | storage, policy |
| validation | build/test/E2E/review gates | execution, workspace, evidence, storage |
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

No dependency may exist only in prose after implementation begins; CI must compare manifests with this matrix.
