# Mayasaba Decision Register

This file is a human-readable register of currently locked design decisions. It is intentionally concise; detailed semantics live in the canonical subsystem documents.

| ID | Decision | Status |
|---|---|---|
| DEC-001 | Project name is Mayasaba | HARD_LOCK |
| DEC-002 | pavan53732/Mayasaba is the canonical implementation repository | HARD_LOCK |
| DEC-003 | Windows-only | HARD_LOCK |
| DEC-004 | Local-only execution; no cloud/remote executor | HARD_LOCK |
| DEC-005 | Initial supported agents are Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, Cline | SUPERSEDED_BY_DEC-029 |
| DEC-006 | Mayasaba is a control plane, not a fifth AI brain | HARD_LOCK |
| DEC-007 | Agent communication is brokered by MCF-v2 | HARD_LOCK |
| DEC-008 | MCF-v2 uses at-least-once delivery + idempotency | HARD_LOCK |
| DEC-009 | SQLite is Mayasaba's durable orchestration source of truth | HARD_LOCK |
| DEC-010 | Context uses project epochs/snapshots/digests | HARD_LOCK |
| DEC-011 | Tasks use leases and explicit ownership | HARD_LOCK |
| DEC-012 | Git worktrees are preferred for parallel agent isolation | HARD_LOCK |
| DEC-013 | Completion is evidence-backed and controller-certified | HARD_LOCK |
| DEC-014 | Desktop stack: Tauri 2 + React 19 + TypeScript + Vite + Tailwind/shadcn | HARD_LOCK |
| DEC-015 | Core stack: Rust + Tokio | HARD_LOCK |
| DEC-016 | User-facing distribution is MSI only | HARD_LOCK |
| DEC-017 | One canonical owner per subsystem; no shadow sources of truth | HARD_LOCK |
| DEC-018 | Protocol schemas use JSON Schema Draft 2020-12 | HARD_LOCK |
| DEC-019 | No private chain-of-thought in Control Room | HARD_LOCK |
| DEC-020 | Project-local `.mayasaba` is portable manifest/import-export only; SQLite app-data remains the sole runtime source of truth | HARD_LOCK |
| DEC-021 | MCF-v2 and Tauri bridge identifiers use machine-readable canonical registries | HARD_LOCK |
| DEC-022 | The implementation target uses exactly 12 Rust crates defined by WORKSPACE-MANIFEST.md | HARD_LOCK |
| DEC-023 | MCF ACK is receipt/persistence only; processing success/failure is reported separately | HARD_LOCK |
| DEC-024 | Privileged shell/process/package/install/admin execution is controller-mediated through ExecutionService; scoped agent file edits remain subject to lease/workspace policy | HARD_LOCK |
| DEC-025 | Context state digests use SHA-256 over RFC 8785 JCS canonicalized authoritative scope input | HARD_LOCK |
| DEC-026 | Bus remains a transport/routing subsystem; PolicyService authorization is completed before material dispatch rather than adding a bus→policy dependency | HARD_LOCK |
| DEC-027 | Material-action idempotency is keyed by project_id + operation_id; message_id identifies individual delivery records | HARD_LOCK |
| DEC-028 | Mayasaba supports user-authorized local-file work across artifact types within a selected workspace; requested public-web research is read-only; external side effects and general control of unrelated applications are out of scope | HARD_LOCK |
| DEC-029 | The supported agent set is exactly Hermes Agent CLI, Kilo Code CLI and OpenCode CLI | HARD_LOCK |

## DEC-029 supersession record

Classification: REPLACEMENT. Supersedes DEC-005.

| Field | Value |
|---|---|
| Previous behavior | DEC-005 locked the agent set to exactly four CLIs: Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, Cline. Each had an `agent_type` wire identity, an adapter-capability record and a native transport contract. |
| New behavior | The agent set is exactly three CLIs: Hermes Agent CLI (`HERMES_AGENT`), Kilo Code CLI (`KILO_CODE`), OpenCode CLI (`OPEN_CODE`). `CLAUDE_CODE` and `CLINE` are removed from every machine-readable registry. Hermes and Kilo are unchanged. |
| Reason | Product decision to restrict Mayasaba's coordination surface to the three retained CLIs. OpenCode CLI is admitted as a new agent; Claude Code CLI and Cline are withdrawn from the product. |
| Compatibility impact | Breaking wire-contract change. `agent_type` is a closed enum in exactly six schemas — `agent-adapter-v1/adapter-types`, `agent-adapter-v1/native-event`, `agent-adapter-v1/probe-result`, `doctor-v1/doctor-report`, `mcf-v2/handshake` and `mcf-v2/identity` — so any retained handshake, native event, probe result or doctor report carrying `CLAUDE_CODE` or `CLINE` becomes invalid. (`mcf-v2/identity` also admits `null`, because it is a shared envelope actor descriptor in which a `MAYASABA` actor has no agent type.) Native transport identifiers `CLAUDE_STREAM_JSON` and `CLINE_JSON` are removed. |
| Migration/reconciliation | No data migration is required. `agents.agent_type` in `schemas/sqlite-v1/schema.sql` is `TEXT NOT NULL` with no CHECK constraint, so SQLite does not constrain the value, and no Mayasaba database has been created yet. There are no installed adapters and no persisted sessions to reconcile. Reject-with-diagnosis is the only runtime path for a legacy `CLAUDE_CODE` or `CLINE` value. |
| Tests affected | Contract verification (`tools/contracts/verify.mjs`) must pass. MCF-v2 fixtures that assert agent identity must use a retained `agent_type`. Adapter conformance fixtures are required for `HERMES_AGENT`, `KILO_CODE` and `OPEN_CODE` before any adapter is admitted. |

Scope note: this decision governs the agent set Mayasaba coordinates at runtime. It does not change which coding agents may contribute to this repository. `AGENTS.md` §1 applicability and the contributor rules in `AGENTS.md` are unchanged by DEC-029.

## DEC-029 verification annex

Classification: ADDITIVE. Date: 2026-10-03. This annex records the evidence that admitted the three-CLI set and hardened its adapter contract. It adds no new decision and does not alter the supersession record above.

The set was admitted against three checks, run before the adapter contract was hardened.

| Check | Result |
|---|---|
| Headless mode exists and is scriptable | Pass for all three. Each accepts a prompt non-interactively and emits a structured stream: `hermes chat -q <prompt> --format stream-json`, `kilo run <prompt> --format json`, `opencode run <prompt> --format json`. Hermes was verified first-hand by running a task headlessly and capturing the raw JSONL. |
| Event streams normalize to the adapter schemas | Pass with schema changes. Kilo and OpenCode emit event kinds the `native_kind` enum did not yet cover — `REASONING`, `STEP_START` and `STEP_FINISH`. Hermes's stream-json records (`system/init`, `text`, `tool_use`, `tool_result`, terminal `result`) all map onto kinds the enum already had, so Hermes forced no schema change. The enum was extended from 12 to 15 values, exactly matching the three additions. |
| Actively maintained | Pass for all three. All three show current commit activity and release cadence. |

Findings that changed the contract, each recorded in `schemas/agent-adapter-v1/native-transport-contract.json`:

| Finding | Consequence |
|---|---|
| Kilo Code CLI is a fork of OpenCode CLI. | Corroborated three ways: 747 `opencode` strings in the Kilo binary including `@opencode/LLMClient` and `@opencode/FileSystem`; `kilocode_change` markers in `bin/kilo`; and vendor documentation stating "The Kilo CLI is a fork of OpenCode and supports the same configuration options." Kilo and OpenCode therefore do not provide independent corroboration in council deliberation; model/provider diversity is unaffected. This weakens the CLI-independence argument for the three-CLI set but not the set itself. |
| Kilo ingests `~/.claude/*` by default. | The adapter must set the six `KILO_DISABLE_*` variables. `OPENCODE_DISABLE_CLAUDE_CODE` has no effect on the Kilo binary. |
| OpenCode 2.x has no workspace flag, and its `run` path resolves the root from `PWD` without changing directory. | A version gate was added: `1.x` admitted, `2.x` unverified. The adapter branches on the probed version and fails closed on an unclassifiable one. On 2.x it must supply the workspace as the child working directory **and** set or clear `PWD`, because an inherited stale `PWD` would otherwise point the agent outside the authorized workspace. Verified against the `v2.0.22` tag; the earlier wording ("2.x rejects `--dir`") was replaced as unsupported. |
| Hermes defaults `updates.check` to true and reaches the network on startup. | `updates.check: false` became a required launch precondition. No launch flag disables it and `--safe-mode` is a trap: it implies `--ignore-user-config`, blanks the model and exits 1. |
| Hermes ACP cannot suppress context injection. | ACP was dropped for Hermes; `stream-json` is its sole transport. |
| Session upload has three independent triggers in OpenCode and Kilo (config `share: "auto"`, the `*_AUTO_SHARE` env var, and the `--share` flag), not one. | Blocking the flag alone was insufficient. Both agents now require `share: "disabled"` in config — a string enum, not a boolean — plus the env var, and Kilo additionally requires its own hard kill switch `KILO_DISABLE_SHARE=1`. |

Known limitation: Claude Code CLI's `claude -p --output-format stream-json` is the most mature headless interface of the four considered, so the three retained adapters carry more pioneering work than one built on it would. The adapter architecture is not a one-way door — locking three agents now does not forbid a Claude Code adapter later, which would be a new governed change to DEC-029.

## Change procedure

Changing a HARD_LOCK requires:

1. explicit change request
2. impact analysis
3. affected decision/contract identification
4. council review where material
5. new version/supersession record
6. migration/reconciliation plan
7. updated tests/evidence
