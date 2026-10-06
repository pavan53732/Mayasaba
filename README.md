# Mayasaba

Mayasaba is a Windows-only, local-first workspace control plane for user-authorized work on files on the user's PC. Software engineering remains first-class, alongside document work, research reports, data cleanup, and code refactors. Mayasaba coordinates Hermes Agent CLI, Kilo Code CLI, and OpenCode CLI through a deterministic council, communication fabric, task engine, execution kernel, validation, repair, and evidence-backed delivery pipeline.

## Canonical repository

**This repository is the only canonical implementation repository for Mayasaba:**

https://github.com/pavan53732/Mayasaba

All Mayasaba source code, architecture, specifications, protocol schemas, tests, design decisions, and implementation history belong here.

## What Mayasaba is

Mayasaba is a **control plane for three CLI agents**. It can coordinate user-authorized tasks on local files across supported task types; the available agent capabilities and evidence determine what can be completed and verified.

It is not a fifth AI brain. Each supported CLI keeps its own model/provider, context, reasoning, tools, authentication and runtime. Mayasaba supplies the shared deterministic workspace state, communication, orchestration, execution, validation and evidence systems.

## Supported agents

1. Hermes Agent CLI
2. Kilo Code CLI
3. OpenCode CLI

The architecture is adapter-based for future extension, but changing this set is a governed change to DEC-029, not a documentation edit.

## Locked boundaries

- Windows-only.
- Local-only execution on the user's Windows PC.
- No cloud VM, remote executor or hosted development workspace.
- No required Mayasaba login/account.
- The user-selected local workspace and task-scoped allowed paths are authoritative; Mayasaba does not scan the whole PC by default.
- User-requested research may retrieve public web sources in read-only mode; reports and citations are saved locally.
- External side-effect actions (email/messages, posts, public/external form submissions, purchases, account changes) and general control of unrelated applications are outside product scope.
- Git worktrees/branches are preferred for concurrent agent isolation.
- SQLite is the durable Mayasaba source of truth.
- Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui.
- Control Room visual language: minimal/functional base with bento-grid layout (DEC-032).
- Rust + Tokio for the controller/core.
- User-facing distribution is MSI only.
- Completion must deliver the requested local artifacts with evidence; software tasks require a working, validated software deliverable rather than planning alone.

## Architecture at a glance

The execution/validation chain below depicts the software-engineering path; other local artifact tasks use the applicable checks described in the canonical specification.

~~~text
USER
  ↓
CONTROL ROOM
  ↓
MAYASABA ORCHESTRATOR
  ↓
MCF-v2 COMMUNICATION FABRIC
  ↓
┌──────────────┬──────────────┬──────────────┐
│ Hermes Agent │ Kilo Code    │ OpenCode     │
└──────────────┴──────────────┴──────────────┘
                         ↓
             WORKSPACE + EXECUTION
                         ↓
        BUILD → TEST → E2E → REVIEW
                         ↓
               DIAGNOSE → REPAIR
                         ↓
                REGRESSION → CERTIFY
                         ↓
                     PACKAGE
~~~

## Canonical software-engineering lifecycle

The lifecycle below is the full software-engineering workflow. Other local artifact tasks use only applicable planning, editing, review, validation and evidence stages; they do not inherit irrelevant software build/package gates.

~~~text
PROJECT_CREATED
→ DISCOVERY
→ INDEPENDENT_ANALYSIS
→ PROPOSALS
→ CROSS_CRITIQUE
→ REBUTTAL_AND_REVISION
→ DISAGREEMENT_RESOLUTION
→ USER_INTERVIEW
→ PRODUCT_AND_UX_DESIGN
→ TECH_STACK_DEBATE
→ ARCHITECTURE_REVIEW
→ ARCHITECTURE_LOCKED
→ TASK_PLANNING
→ IMPLEMENTATION
→ INTEGRATION
→ BUILD
→ TEST
→ E2E
→ CROSS_AGENT_REVIEW
→ REPAIR
→ FINAL_VALIDATION
→ PACKAGE
→ COMPLETE
~~~

REPAIR is entered only when required. COMPLETE is controller-owned and evidence-backed.

## Canonical subsystem ownership

| Responsibility | Canonical owner |
|---|---|
| Protocol schemas | crates/protocol |
| Communication bus | crates/bus |
| Agent adapters/sessions | crates/agents |
| Orchestration/state transitions | crates/core |
| Council | crates/council |
| Context synchronization | core context orchestration |
| Tasks and leases | crates/tasks |
| Workspace/worktrees | crates/workspace |
| Local execution | crates/execution |
| Validation | crates/validation |
| Evidence/artifacts | crates/evidence |
| Security/policy | crates/policy |
| SQLite persistence | crates/storage |

No subsystem may create a competing authority for another subsystem.

## MCF-v2

MCF-v2 is the canonical communication protocol connecting Mayasaba and the three CLIs.

It provides:

- typed versioned envelopes
- at-least-once delivery
- ACK/NACK
- idempotent side effects
- ordering and causality
- retries and dead letters
- priority lanes and backpressure
- project isolation
- project epochs
- ContextPacks and stale-context protection
- task leases
- proof-carrying handoffs
- council barriers
- pause/resume/cancel/stop propagation
- transactional outbox and receiver inbox/deduplication
- replay and crash recovery

**ACK is receipt, not success. An agent report is not authoritative truth.**

## Truth hierarchy

1. Current user-approved requirements
2. HARD_LOCK decisions
3. Versioned architecture/contracts
4. Verified repository/workspace facts
5. Objective task-appropriate evidence (build/test/E2E for software where applicable; citations and integrity checks for other artifact tasks)
6. Persisted Mayasaba orchestration state
7. Agent proposals/reports

## Documentation

Start with docs/INDEX.md.

The documentation is intentionally split by ownership to avoid one oversized, conflicting specification.

## Repository layout

~~~text
mayasaba/
├── apps/desktop/
├── crates/
│   ├── core/
│   ├── protocol/
│   ├── bus/
│   ├── agents/
│   ├── council/
│   ├── execution/
│   ├── workspace/
│   ├── tasks/
│   ├── validation/
│   ├── evidence/
│   ├── policy/
│   └── storage/
├── schemas/mcf-v2/
├── docs/
├── tests/
├── scripts/
└── installer/
~~~

## Documentation governance

Architecture changes must be classified as ADDITIVE, REFINEMENT, REPLACEMENT or DEPRECATION. Existing canonical terms, schemas, state owners and sources of truth must not be silently redefined.

## Development setup

Windows, from the repository root. Node 22+ and a Rust toolchain (Cargo >= 1.78, because `Cargo.lock` is
version 4) are required.

~~~text
git clone https://github.com/pavan53732/Mayasaba
cd Mayasaba
npm install
git config core.hooksPath .githooks
npm run verify:contracts
npm --prefix apps/desktop test
cargo test --workspace
cargo fmt --all --check
~~~

`git config core.hooksPath .githooks` is a per-clone opt-in: git cannot enable a hook on clone, so a fresh
clone commits without the gate until it is set. `cargo fmt` and `cargo clippy` are separate rustup components
and are not installed by a default toolchain:

~~~text
rustup component add rustfmt clippy
~~~

`cargo fmt --all --check` skips the two files under `crates/protocol/src/generated/`, because the contract
gate compares them byte-for-byte with `tools/codegen/generate-protocol.mjs`'s output and rustfmt would rewrap
them — the generator owns that formatting, and `#[rustfmt::skip]` on the module declarations says so.

`git blame` can also skip the repo-wide formatting commit, which otherwise attributes every reformatted line
to it rather than to the change that wrote it:

~~~text
git config blame.ignoreRevsFile .git-blame-ignore-revs
~~~

## Local validation

Contract verification runs locally on the user's Windows PC — there is no hosted CI (DEC-036). From the repository root:

~~~text
npm run verify:contracts
~~~

This runs `tools/contracts/verify.mjs`, which checks the MCF registry and protocol/message/event enums, the Tauri bridge identifiers against their declared owners in `workspace.manifest.json`, both sides of the Tauri bridge — the registered handlers in `apps/desktop/src-tauri/src/main.rs` and the frontend's `transport(...)` call sites — each handler's arguments against the operation's declared request fields, the canonical error registry against the protocol's own error enums and against the codes the implementation produces, the generated bridge and protocol surfaces against the contract they are derived from, the agent-adapter set and declared controls, the existence of the declared manifests, and that each crate's `Cargo.toml` names its declared `mayasaba-*` dependencies. A non-zero exit means contract drift; it must be resolved before handoff, not waived. It requires no network access and no CI service.

The payload types the contract declares are checked separately, by the shell's own test suite rather than by the gate: `cargo test -p mayasaba-desktop` runs a conformance test that serializes each implemented operation's response struct and validates it against its declared type in `schemas/tauri-bridge-v1/payload-types.json`, so a renamed, added, removed or recased wire field fails by name.

The checks have tests of their own. `npm run verify:contracts:mutations` reintroduces, one at a time, the drift each check exists to catch, and requires the check to fail and to name that specific disagreement:

~~~text
npm run verify:contracts:mutations
~~~

It mutates the working tree, so it refuses to start when a tracked file is modified and restores with git; it is deliberately not part of `npm run verify:contracts` or the pre-commit hook, because it is a tool to run when a check changes rather than on every commit (DEC-057).

The gate is wired to a version-controlled pre-commit hook, so it is not merely available but run:

~~~text
git config core.hooksPath .githooks
~~~

`.githooks/pre-commit` lives in the repository, so there is one copy and nothing to install or keep in sync — `core.hooksPath` is a pointer, not a copy, and `git config core.hooksPath` reports the whole state of the mechanism. The gate prints whether the hook is enabled in its summary line. That report is informational and never a failure, so a contributor who deliberately opts out is not blocked.

### Full local verification

The contract gate validates contracts. It never compiles or tests Rust, so a green gate and a broken build are entirely compatible — which is how a workspace with 104 build errors reached `main` while the gate reported success. `npm run verify:local` closes that scope gap:

~~~text
npm run verify:local
~~~

It runs, in order, the contract gate, `cargo fmt --all --check`, `cargo build --workspace --all-targets`, `cargo test --workspace` and the desktop test suite, stops at the first failure, and prints a per-step summary. `npm run verify:rust` runs only the three Rust steps, and `--only=<group|id>` narrows the selection further (`--list` names the groups and ids).

`tools/verify/local.mjs` **refuses to run on any platform other than Windows**. That is deliberate rather than incidental. Mayasaba is Windows-only and local-first (AGENTS.md §3; DEC-003, DEC-004), and DEC-036 makes verification a gate on the user's own Windows PC. A hosted runner — including a Windows-hosted one — moves execution off that machine, so it violates the boundary rather than satisfying it. GitHub is the source repository, history and code-review surface only; it runs nothing. For the same reason the repository carries **no `.github/` tree at all, permanently** (DEC-036, DEC-105) — and that ban is enforced rather than merely documented. The contract gate fails, naming every offending path, on any file present under `.github/`, so re-adding a workflow is caught at the commit that would introduce it rather than discovered later. `npm run verify:contracts:mutations` proves the check fails by creating a workflow and requiring the gate to reject it by name.

This runner covers build, tests and gates. MSI packaging, and any runtime or end-to-end exercise of the installed application, remain separate local steps on the same machine.

## Current status

Verified on 2026-10-05 as part of the DEC-053 bridge reconciliation and the DEC-054, DEC-055 and DEC-056
follow-ups; each decision record states the exact validation performed and the open items it left. The
machine-readable contract is packaged and gated: `npm run verify:contracts` checks the MCF registries and enums,
the Tauri bridge identifiers against their declared owners in `workspace.manifest.json`, the agent-adapter set
and its declared controls, the declared manifests, each crate's declared dependencies, the canonical error
registry against the protocol's own error enums and against the codes the implementation produces, both
sides of the Tauri bridge, and each handler's arguments against the operation's declared request fields. It
reports 59 MCF message types, 121 events, 12 state machines, 32 Tauri commands, 27
queries, 30 UI events and 31 registered error codes, of which 11 are produced by the implementation. It
invariant-checks 28 of the 68 canonical artifacts; the other 40 are parsed but have no invariant enforced
against them.

What is implemented:

- A working vertical slice through Tauri into SQLite. Four handlers are registered in
  `apps/desktop/src-tauri/src/main.rs` (`generate_handler!`) and called by the Control Room intake surface:
  `create_project`, `list_projects`, `get_recovery_status` and `validate_workspace`. All four names are
  declared in the bridge contract — `create_project` (a declared command) and `list_projects`,
  `get_recovery_status` and `validate_workspace` (declared queries). The contract also still declares
  `validate_configuration`, which nothing implements. The remaining 31 declared commands and 24 declared
  queries have no handler. The gate reads both sides: it parses the `#[tauri::command]` functions and the
  `generate_handler![...]` list out of `main.rs`, and the `transport(...)` call sites out of the frontend, so a
  handler or a call that names an undeclared operation fails the gate, while a declared operation that nothing
  implements is reported with its count rather than failed. DEC-053 records the drift this closed. Every field
  these handlers carry crosses the boundary in the contract's own snake_case spelling — the wire structs
  serialize with `#[serde(rename_all = "snake_case")]` and the commands take their arguments with
  `#[tauri::command(rename_all = "snake_case")]`, so the frontend renames nothing and no translation layer has
  to be kept in step with Rust by hand (DEC-054). Every error code they can return is registered with its
  category, retryability, severity, MCF spelling and meaning, and the gate fails if the implementation produces
  a code the registry does not declare (DEC-055). What each of the four returns is declared in
  `schemas/tauri-bridge-v1/payload-types.json` and checked: `mod wire_shape_tests` in `main.rs` serializes every
  one of these structs and validates it against the type its operation declares, and the gate compares each
  handler's arguments with the operation's declared request fields, so a field that is renamed, added, removed
  or recased on either side fails by name instead of reaching the frontend as `undefined` (DEC-056).
- MCF-v2 envelope validation in `crates/protocol`, with the envelope's vocabulary and per-field JSON types
  generated from the contract rather than hand-copied (DEC-051), plus startup recovery and durable-state
  inconsistency reporting in `crates/storage`.
- The deterministic council decision-quality logic in `crates/council` (DEC-052): mode selection, evidence
  grading, lineage-group corroboration, round roles, synthesis coverage and review, budget accounting, and
  append-only decision-outcome records. There is **no council runtime** — nothing here spawns an agent, and no
  round is orchestrated end to end.

Crate sizes are not used as a maturity metric. The current runtime baseline includes durable storage,
task/lease orchestration, recovery planning, workspace admission/checkpoints, the contract-driven agent gateway,
local process supervision and deterministic council decision-quality logic. The remaining milestones are tracked by
behavioral gates in `docs/ROADMAP.md`, not by line count; policy, live transport dispatch, Git worktree/integration,
evidence/validation completion, Control Room, build/test/E2E and MSI packaging remain staged.

Per `docs/ROADMAP.md` the milestones are ordered M0 → M0.5 → M1 → M2 → M2.5 → M3 → M4 → M4.5 → M5 → M6 → M7 → M8 → M9 → M10 → M11. M0, M0.5, M1 and M2 are complete; **M2.5 shell integration is in progress** and does not yet dispatch to live adapters. Council decision-quality logic has landed ahead of the live adapter/runtime milestones because it is deterministic logic with no live-CLI dependency.


### Long-running orchestration reliability

The reliability model is additive to the existing MCF-v2/control-plane architecture. The stable acceptance unit is `Task`; retries are `TaskAttempt`s fenced by the active `TaskLease.lease_version`. Local scarce resources are durably admitted, workspace and environment observations are revisioned, safe points use `WorkspaceCheckpoint(kind=SAFE_POINT)`, and certification binds exact validated inputs. Project remains the durable lifecycle root; `events` remain the journal; no duplicate Mission/Worker/Cell authority is introduced.

Current repository status: M2 durable bus is implemented; M2.5 shell integration is in progress. The supported product agent set remains exactly Hermes Agent CLI, Kilo Code CLI and OpenCode CLI (DEC-029).
