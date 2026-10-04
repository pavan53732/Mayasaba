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

This runs `tools/contracts/verify.mjs`, which checks the MCF registry and protocol/message/event enums, the Tauri bridge identifiers against their declared owners in `workspace.manifest.json`, the agent-adapter set and declared controls, the existence of the declared manifests, and that each crate's `Cargo.toml` names its declared `mayasaba-*` dependencies. A non-zero exit means contract drift; it must be resolved before handoff, not waived. It requires no network access and no CI service.

The gate is wired to a version-controlled pre-commit hook, so it is not merely available but run:

~~~text
git config core.hooksPath .githooks
~~~

`.githooks/pre-commit` lives in the repository, so there is one copy and nothing to install or keep in sync — `core.hooksPath` is a pointer, not a copy, and `git config core.hooksPath` reports the whole state of the mechanism. The gate prints whether the hook is enabled in its summary line. That report is informational and never a failure, so a contributor who deliberately opts out is not blocked.

## Current status

Verified on 2026-10-05 as part of the DEC-053 bridge reconciliation; the decision record states the exact
validation performed and the open items it left. The machine-readable contract is packaged and gated: `npm run
verify:contracts` checks the MCF registries and enums, the Tauri bridge identifiers against their declared
owners in `workspace.manifest.json`, the agent-adapter set and its declared controls, the declared manifests,
and each crate's declared dependencies. It reports 59 MCF message types, 121 events, 12 state machines, 32
Tauri commands, 27 queries and 30 UI events, and it invariant-checks 27 of the 68 canonical artifacts; the
other 41 are parsed but have no invariant enforced against them.

What is implemented:

- A working vertical slice through Tauri into SQLite. Four handlers are registered in
  `apps/desktop/src-tauri/src/main.rs` (`generate_handler!`) and called by the Control Room intake surface:
  `create_project`, `list_projects`, `get_recovery_status` and `validate_workspace`. All four names are
  declared in the bridge contract — `create_project` (a declared command) and `list_projects`,
  `get_recovery_status` and `validate_workspace` (declared queries). The contract also still declares
  `validate_configuration`, which nothing implements. The remaining 31 declared commands and 25 declared
  queries have no handler. The gate reads the contract and never reads `main.rs` or the frontend's
  `transport(...)` calls, so a handler whose name the contract does not declare is invisible to it; DEC-053
  records this drift and the gate change that closes it.
- MCF-v2 envelope validation in `crates/protocol`, with the envelope's vocabulary and per-field JSON types
  generated from the contract rather than hand-copied (DEC-051), plus startup recovery and durable-state
  inconsistency reporting in `crates/storage`.
- The deterministic council decision-quality logic in `crates/council` (DEC-052): mode selection, evidence
  grading, lineage-group corroboration, round roles, synthesis coverage and review, budget accounting, and
  append-only decision-outcome records. There is **no council runtime** — nothing here spawns an agent, and no
  round is orchestrated end to end.

Crate sizes at this commit, which is the honest read on what is real: `council` ~6,800 lines, `storage` ~2,100,
`protocol` ~1,500 (about 330 of it generated), `core` ~460, `workspace` ~290. Seven crates are still two-line
stubs: `agents`, `bus`, `evidence`, `execution`, `policy`, `tasks` and `validation`.

Per `docs/ROADMAP.md` the milestones are ordered M0 → M0.5 → M1 → M2 → M3 → M4 → M4.5 → M5 → M6 → M7 → M8 →
M9. M0, M0.5 and M1 are complete; **M2, the durable local communication bus, is the next unstarted
milestone**, and `crates/bus` is still a stub. The council logic is M4.5/M6 work that has landed ahead of M2
because it is pure logic with no runtime dependency on the bus.
