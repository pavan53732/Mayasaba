# Mayasaba Agent Integration

## Supported agents

Support is exactly:

1. Hermes Agent CLI
2. Kilo Code CLI
3. OpenCode CLI

Future agents require an explicit architecture/protocol extension and are not silently added. Withdrawing or adding an agent is a governed change to DEC-029.

## Adapter contract

Every adapter exposes:

- detect()
- version()
- capabilities()
- health_check()
- launch()
- send()
- stream()/receive()
- interrupt()
- resume()
- stop()
- collect_changes()
- collect_evidence()

## Adapter responsibilities

An adapter:

- discovers its CLI/runtime
- launches and supervises the local process
- translates native input/output into MCF-v2
- maintains session identity
- exposes runtime capability facts
- handles structured streaming
- confirms actual process state
- collects changed-file/evidence references
- isolates native quirks from Mayasaba core

An adapter does not own project state, task truth or certification.

## Project intent in the ContextPack

The ContextPack delivered before a task lease carries the `ProjectBrief` version that is the analysis anchor for the lineage (see `MEMORY-CONTEXT.md` and `DATA-MODEL.md`). The brief is controller-owned project truth, delivered through the adapter's structured input or instruction mechanism like any other runtime policy — never left to native file discovery. Adapters must preserve the brief's version reference so resume and ContextPack regeneration do not silently substitute a different version.

## Runtime policy and workspace-instruction boundary

Mayasaba distinguishes two instruction planes:

1. **Runtime policy and task context** are controller-owned. They come from authoritative project/task state, the active lease and policy decisions, and the applicable product-owned contracts. They apply to every supported adapter and are delivered before a task lease.
2. **Workspace instructions** are guidance found in the selected target workspace (for example, that workspace's `AGENTS.md` or any other instruction file the workspace ships). They are scoped to that workspace and task; they may refine how work is performed but cannot override Mayasaba policy, grant capabilities or expand the lease scope.

The controller MUST deliver the same applicable runtime policy meaning and task context to Hermes Agent CLI, Kilo Code CLI and OpenCode CLI. Use each CLI's supported structured input or instruction mechanism; never depend on native file discovery to enforce Mayasaba policy. **Every** supported CLI discovers instruction files outside the authorized workspace by default, and each adapter must disable that discovery explicitly:

- **Hermes** injects `AGENTS.md` (git-root to cwd chain), `SOUL.md` from `HERMES_HOME`, memory, `.cursorrules` and preloaded skills — suppress with `--ignore-rules`.
- **Kilo** (an OpenCode fork) ingests `~/.claude/CLAUDE.md`, `~/.claude/skills` and `~/.claude.json` — suppress with the `KILO_DISABLE_CLAUDE_CODE*` variables.
- **OpenCode** ingests `~/.claude/CLAUDE.md`, `.claude` prompt content and `.claude/skills` — suppress with `OPENCODE_DISABLE_CLAUDE_CODE=1`.

An adapter that cannot suppress out-of-scope instruction discovery must fail admission rather than rely on the discovered files being harmless.

- The Mayasaba repository's root `AGENTS.md` is contributor guidance for AI coding agents working in that repository. It is the single canonical repository instruction file; there is no agent-specific overlay. It is not universal runtime policy or configuration for the adapters embedded in Mayasaba.
- If a task's selected workspace is the Mayasaba repository, its repository instruction files may be supplied as task-scoped workspace guidance. Preserve their source and scope. Runtime policy remains authoritative if instructions conflict.
- A session MUST NOT reach `READY` or receive a task lease until delivery of the required runtime policy is confirmed. Preserve policy source/version references with the session/task context so resume and ContextPack regeneration do not silently omit them.
- For Mayasaba architecture tasks, supply the task-scoped co-design instruction and relevant accepted decisions, HARD_LOCKs, open proposals, and canonical owner-document/schema references in the ContextPack as defined by `MEMORY-CONTEXT.md`; do not treat root `AGENTS.md` §14 as a universal runtime-policy source.

`MEMORY-CONTEXT.md` defines the ContextPack contents and existing-field mapping. These requirements do not authorize changes to the MCF-v2 envelope or payload schema.

## Product-scope boundary

Agents work on user-authorized files within the leased workspace, which may include source code, documents, research reports and datasets. Public-web access is permitted only as read-only retrieval for a user-requested research task; any output is saved locally with citations/source metadata.

No agent capability or native CLI feature authorizes sending email/messages, posting to external services, submitting forms to public/external services, making purchases, changing accounts or generally controlling unrelated applications. These restrictions apply even when an installed CLI advertises browser, terminal or general automation capabilities. Runtime capability probes report what a CLI can do; policy determines what Mayasaba allows.

## Native protocol boundary

Where a CLI supports a structured machine transport, use it rather than ANSI scraping. ANSI scraping is a fallback only.

The adapter is responsible for normalizing:

~~~text
native protocol → adapter → canonical MCF-v2 → bus
~~~

and:

~~~text
MCF-v2 → adapter → native protocol
~~~

## Agent session lifecycle

~~~text
DISCOVERED → HANDSHAKING → CAPABILITY_VALIDATING → WORKSPACE_VALIDATING → READY → ACTIVE → PAUSED → DRAINING → STOPPED
~~~

Failure/recovery:

~~~text
READY/ACTIVE → LOST → RECONNECTING → SYNCING → READY/ACTIVE
~~~

Terminal adapter failure:

~~~text
RECONNECTING → FAILED
~~~

A task lease is not issued until READY.

## Capability negotiation

Capabilities are runtime-detected, including where applicable:

- structured I/O
- streaming
- interruption
- resume
- autonomous execution
- working directory
- file write
- execution request via Mayasaba ExecutionService
- native terminal/shell use is admitted only when the adapter probe proves it can be restricted or fully supervised
- read-only retrieval/rendering of public web sources for user-requested research, if explicitly supported and policy-gated; no authenticated interaction with public/external services or side-effecting public-web actions. Scoped UI automation may validate the selected local target app for software tasks when it causes no external side effects.
- Git
- patching
- artifact reporting
- structured errors

A documentation claim is not a runtime capability fact.

## Agent isolation

Each agent session is bound to:

- project
- session
- agent type
- workspace/worktree
- lease
- context snapshot
- policy scope

An agent must not modify another agent's isolated workspace.

## Session failure

On loss:

1. mark session LOST
2. preserve event history
3. reconcile child processes
4. verify workspace/checkpoint
5. reconcile or expire leases
6. reconnect or reassign
7. regenerate current ContextPack
8. require all action gates before resuming


Concrete adapter behavior remains runtime-probed; native CLI quirks stay inside each adapter. The implementation-level process/execution boundary is defined in `EXECUTION-KERNEL-DESIGN.md`.

## Native CLI integration facts

The following are documented baseline facts. Runtime probing remains authoritative for the installed executable/version, so adapter configuration must record the probe result.

### Hermes Agent CLI

- executable: `hermes`
- non-interactive entry point: `hermes chat -q <query>`
- structured output: `--format stream-json`
- stream format is JSONL on stdout; diagnostics and the session-id line are on stderr
- terminal record is `result`, including interrupted runs
- session resume: `--resume <native_session_id>`
- stream-json is the **sole** Hermes transport. Hermes has no admissible ACP path: its ACP entry point builds the agent without `skip_context_files`/`skip_memory`, and `HERMES_IGNORE_RULES` does not gate that path, so ACP cannot suppress workspace/memory context injection. ACP is therefore not admitted for Hermes until Hermes exposes a flag that gates context injection on that path.
- Hermes injects `AGENTS.md` (git-root to cwd chain), `SOUL.md` from `HERMES_HOME`, memory, `.cursorrules` and preloaded skills by default. The adapter MUST pass `--ignore-rules` to suppress that injection, so no instruction file outside the authorized workspace can act as an instruction source.
- Hermes defaults `updates.check` to true and issues an outbound update check on chat startup. No launch flag disables this and `--safe-mode` does not either; the adapter MUST require `updates.check: false` (set via `hermes config set updates.check false`) before launch, because a local-first product must not ship an adapter whose default startup reaches the network.
- The adapter MUST NOT pass `--yolo` or `--accept-hooks`; both bypass approval prompts and would contradict controller-mediated execution.
- Hermes exposes outbound-messaging, service and credential-injection subcommands (`send`, `slack`, `whatsapp`, `whatsapp-cloud`, `webhook`, `peer`, `gateway`, `portal`, `egress`, `proxy`, `cron`, `kanban`, `sync`). Mayasaba MUST NOT invoke them.

Hermes documents stream-json CLI output and its configuration surface. Source: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/reference/cli-commands.md

**Correction (superseded claim):** an earlier revision of this document described `hermes acp` / `hermes-acp` as a supported second transport. That claim is withdrawn — ACP is not admissible for Hermes, as set out above.

### Kilo Code CLI

- executable: `kilo`
- autonomous entry point: `kilo run "<message>" --auto`
- structured output: `kilo run --format json`, which emits a raw newline-delimited JSON **event stream** (observed types include `step_start`, `step_finish`, `text`, `tool_use`, `reasoning` and `error`). There is no session-level start/end record, and a tool call plus its result are fused into a single `tool_use` record.
- **completion semantics:** the process exit code is not the sole completion signal. The adapter must normalize the event sequence, must not assume a single terminal result object, and must derive completion from observed event content. (This mirrors OpenCode, whose `run` is the same lineage.)
- ACP entry point: `kilo acp`
- ACP can use stdio/default local operation; the CLI also exposes host/port options
- `kilo run` supports session continuation (`--session <native_session_id>`) and working-directory selection via `--dir <workspace>`. The adapter MUST pass an **absolute** path: Kilo resolves an absolute `--dir` directly, but joins a relative one onto a root it derives from the environment's `PWD`, so a relative path under a stale `PWD` can resolve outside the authorized workspace.
- Kilo is a **fork of OpenCode** and carries the same cloud/share surface. Mayasaba MUST NOT use Kilo's cloud/remote surface: `cloud`, `remote`, `serve`, `attach`, `github`, `pr`, `daemon`, `import`, nor the `--share`, `--cloud-fork` or `--attach` flags. `--share` uploads the session to a public share URL, `--cloud-fork` fetches a session from cloud, and `--attach` connects to a running kilo server. Only local `run` and local `acp` execution is supported.
- **session sharing has three independent triggers and the adapter must close all three.** The share step runs unless `config.share !== "auto" && !autoShare && !args.share`, so a session is uploaded when the config key `share` is `"auto"`, **or** the `KILO_AUTO_SHARE` env var is truthy, **or** the `--share` flag is passed. Blocking the flag alone is not sufficient. The adapter MUST set `share: "disabled"` in config — a **string** enum (`"manual"` default, `"auto"`, `"disabled"`), not a boolean — and MUST set `KILO_AUTO_SHARE=0`. Kilo also exposes its own hard kill switch, `KILO_DISABLE_SHARE=1`, which the adapter MUST set.
- because Kilo is an OpenCode fork, it ingests `~/.claude/CLAUDE.md`, `~/.claude/skills` and `~/.claude.json` by default — paths outside Mayasaba-authorized scope. The adapter MUST set `KILO_DISABLE_CLAUDE_CODE=1`, `KILO_DISABLE_CLAUDE_CODE_SKILLS=1`, `KILO_DISABLE_CLAUDE_CODE_PROMPT=1`, `KILO_DISABLE_DEFAULT_PLUGINS=1` and `KILO_DISABLE_AUTOUPDATE=1`. Kilo reads the `KILO_`-prefixed variables; `OPENCODE_DISABLE_CLAUDE_CODE` has no effect on the Kilo binary and must not be relied on.
- **permission enforcement (hard admission precondition):** `--auto` is retained in the launch vector, but the adapter MUST fail admission for any task requiring execution mediation unless it first proves that Kilo's resolved permission map denies privileged terminal/command operations. `--auto` approves everything not explicitly denied, so a permissive native config would otherwise let Kilo approve shell execution and bypass the controller gate (DEC-024). Verify the resolved permission map from the probe; do not assume Kilo's defaults are restrictive.

Kilo documents `kilo run`, JSON output, ACP, session controls and cwd options. Source: https://kilo.ai/docs/code-with-ai/platforms/cli

**Correction (overstated attribution):** an earlier revision of this document attributed Kilo's cloud prohibition to a specific named command only (`kilo cloud`). The prohibition is broader — see the list above — and the fork relationship to OpenCode is why Kilo's prohibited surface is kept aligned with OpenCode's.

### OpenCode CLI

- executable: `opencode`
- non-interactive entry point: `opencode run "<message>"`
- structured output: `--format json`, which emits a raw JSON **event stream**; `--format` accepts only `default` or `json` for `run`
- because the output is a typed event sequence rather than a single fixed-shape result document, the adapter must normalize the event sequence and must not assume a single terminal object
- **completion semantics:** the process exit code is non-authoritative. The adapter MUST treat exit-0-with-empty-output as a failure, derive completion from observed event content rather than a terminator record, and preserve subagent-origin parts explicitly because parts are filtered to the root session.
- version gate: the **1.x** line (npm `latest`) is admitted and accepts `--dir <path>`; an absolute path is resolved directly and is the form the adapter must use. The **2.x** line is unverified and is not published to npm as `latest`. The adapter must branch on the probed version and must fail closed on a version it cannot classify.
- **on 2.x there is no workspace flag at all.** `--dir` is not merely rejected: no `dir` flag exists on `run` or on any other 2.x command. The root-level `directory` positional is consumed only by the TUI default handler and by an internal v1 compatibility bridge, not by the native `run` handler. The workspace must therefore be supplied as the child process working directory.
- **working-directory hazard (2.x).** The 2.x `run` path resolves its root as `options.root ?? process.env.PWD ?? process.cwd()` and never changes directory. A spawned child that inherits a stale `PWD` runs against the wrong directory, and `run` offers no flag to override it. The adapter MUST set or clear `PWD` in the child environment in addition to setting the working directory. This is a scope-integrity requirement: an inherited `PWD` can point the agent outside the authorized workspace.
- session resume: `--session <session-id>` (or `--continue`); `--fork` forks rather than continuing in place
- agent selection: `--agent <name>`, where an agent's frontmatter permission map denies anything not explicitly allowed
- autonomous approval: `--auto` approves permissions that are not explicitly denied; Mayasaba MUST NOT pass `--auto` for tasks requiring controller-mediated execution, because approval would bypass the controller gate
- ACP entry point: `opencode acp --cwd <path>`; ACP is newline-delimited JSON-RPC over stdio, protocol version 1, and the process serves multiple sessions until stdin closes
- global flags: `--version`, `--print-logs`, and `--pure` (run without external plugins)
- OpenCode ships `serve`, `web`, `attach` and `github` subcommands; Mayasaba MUST NOT use them, because they start network services or reach external endpoints. Only local `run` and local `acp` are supported, consistent with the local-only constraint.
- **session sharing has three independent triggers and the adapter must close all three.** `share` is **not** a subcommand — it is a boolean flag on `run`. The share step runs unless `config.share !== "auto" && !autoShare && !args.share`, so a session is uploaded when the config key `share` is `"auto"`, **or** the `OPENCODE_AUTO_SHARE` env var is truthy, **or** the `--share` flag is passed. Blocking the token `share` as a subcommand would not prevent `opencode run --share`. The adapter MUST block `--share` as a flag, set `OPENCODE_AUTO_SHARE=0`, and require `share: "disabled"` in config — a **string** enum (`"manual"` default, `"auto"`, `"disabled"`), not a boolean.
- by default OpenCode ingests `~/.claude/CLAUDE.md`, `.claude` prompt content and `.claude/skills`. The adapter MUST set `OPENCODE_DISABLE_CLAUDE_CODE=1` so that files outside Mayasaba-authorized scope can never act as an instruction source. `OPENCODE_DISABLE_DEFAULT_PLUGINS=1` and `OPENCODE_DISABLE_AUTOUPDATE=1` are likewise required to keep adapter behavior deterministic and offline.
- provider credentials live in `~/.local/share/opencode/auth.json`; `opencode auth list` is a readiness probe only. Credentials are external to MCF-v2 and must never be copied into protocol payloads.

OpenCode documents `opencode run`, `--format json`, `--dir`, `--session`, `--agent`, `--auto`, `opencode acp`, global flags and the `OPENCODE_*` environment variables. Source: https://opencode.ai/docs/cli/ and https://opencode.ai/docs/acp/

**Correction (withdrawn claim):** an earlier revision of this document asserted that a `stream-json` output mode for `opencode run` "appears in an unmerged third-party pull request". No source in this repository supports that claim, and it is withdrawn. What is established is the observable constraint: `--format` for `run` accepts only `default` or `json`. The adapter must not depend on any other value, and the runtime probe remains authoritative for the installed version.

**Correction (unsupported claim):** an earlier revision of this document stated that "the **2.x** line rejects `--dir` as an unrecognized flag". That phrasing implies `--dir` was parsed and refused, and rested on no source in this repository. It was re-verified against the `v2.0.22` tag: the `dir` flag is absent from the 2.x command definitions entirely, and the root-level `directory` positional reaches the TUI handler and the internal v1 bridge rather than `run`. The claim is replaced by the two bullets above. The same check confirmed the non-obvious `PWD` resolution hazard on 2.x, which the earlier text did not capture.

## Adapter transport decision

| Agent | Native automation transport | Session model | Adapter strategy |
|---|---|---|---|
| Hermes | stream-JSON stdio only | Hermes session ID | dedicated stream-JSON adapter; **ACP not admitted** |
| Kilo | JSON stdio or ACP | Kilo session ID | dedicated JSON adapter; ACP optional capability |
| OpenCode | JSON event stream stdio or ACP stdio | OpenCode session ID | dedicated JSON event-stream adapter; ACP optional capability |

Mayasaba does not force all three agents through ACP. Native structured transports are preferred when they provide the required capabilities; ACP is selected only where the adapter capability matrix confirms it is the correct runtime path — and for Hermes it is not confirmed, so stream-json is Hermes's sole transport. The authoritative per-agent ACP fact lives in `schemas/agent-adapter-v1/native-transport-contract.json` and `schemas/mcf-v2/conformance/adapter-capabilities.yaml`; this table must not contradict them.

## Runtime probe contract

Every `detect()`/handshake operation produces a typed record conforming to `schemas/agent-adapter-v1/probe-result.schema.json`.

The probe records executable path, version, transport, exact argv, cwd, stdin/stdout/stderr mode, detected capabilities, exit behavior and authentication detection.

Probe facts are versioned per installed executable. Documentation never overrides a failed runtime probe.

## Typed adapter contract

The implementation contract is:

```text
detect() -> Result<AgentInstallation, AdapterError>
version() -> Result<AgentVersion, AdapterError>
capabilities() -> Result<CapabilitySet, AdapterError>
health_check() -> Result<HealthStatus, AdapterError>
launch(SessionLaunch) -> Result<AgentSessionHandle, AdapterError>
send(McfEnvelope) -> Result<SendReceipt, AdapterError>
stream() -> Result<NativeEventStream, AdapterError>
interrupt() -> Result<(), AdapterError>
resume(SessionId) -> Result<AgentSessionHandle, AdapterError>
stop(StopReason) -> Result<(), AdapterError>
collect_changes(ChangeScope) -> Result<ChangeSet, AdapterError>
collect_evidence(EvidenceScope) -> Result<EvidenceRefs, AdapterError>
```

All adapter errors are classified as detection, launch, transport, parse, process, timeout, cancellation, authentication, capability, workspace or protocol errors and include retryability plus causal references.

## Cancellation semantics

Cancellation is controller-owned:

MCF `CANCEL` → adapter cancellation request → native interrupt/termination → process-tree verification → session/task reconciliation.

An adapter may not report cancellation success until the child process state is verified or a bounded cleanup failure is recorded.

## Version drift

Native flags and output schemas are runtime facts, not MCF-v2 contracts. Adapter probes must fail closed when a required structured transport disappears or changes incompatibly.


## Execution mediation

Mayasaba uses a hybrid adapter model:

- scoped file edits may remain native to the agent inside its leased workspace;
- shell/process/package/install/admin operations are controller-mediated through `EXECUTION_REQUEST`;
- native agent terminal capability is therefore not sufficient evidence of Mayasaba execution authority;
- the adapter must reject admission when required execution mediation cannot be enforced.

## Native invocation baseline

The current documented native automation surfaces are runtime facts and must be recorded in the probe result. Hermes supports `hermes chat -q` with `--format stream-json`; Kilo supports `kilo run` with `--format json` and `--auto`; OpenCode supports `opencode run` with `--format json`, `--dir`, `--session` and ACP via `opencode acp`. Runtime probe remains authoritative for the installed version.

The exact launch, resume, version and auth vectors per agent are owned by `schemas/agent-adapter-v1/native-transport-contract.json`. This section is a narrative summary; where it and the contract differ, the contract governs.

Sources: NousResearch Hermes CLI reference; Kilo Code CLI documentation; OpenCode CLI and ACP documentation.

## Native event normalization

Native CLI output is first normalized to schemas/agent-adapter-v1/native-event.schema.json, then translated according to native-to-mcf.registry.json. Runtime probe results remain authoritative for installed-version behavior.

A native kind with no MCF equivalent is dropped or retained as adapter-local telemetry; it must **not** be translated to `ERROR`. `ERROR` is a failure signal, and emitting it for an unmapped progress record would fabricate a failure that did not occur. This is why `REASONING`, `STEP_START`, `STEP_FINISH` and `UNKNOWN` map to `null` in the registry rather than to an error type.

Contract verification enforces that the `native_kind` enum and the registry's mapping keys stay in lockstep, and that every non-null mapping target is a real MCF message type.
