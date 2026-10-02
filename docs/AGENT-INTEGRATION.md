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

## Runtime policy and workspace-instruction boundary

Mayasaba distinguishes two instruction planes:

1. **Runtime policy and task context** are controller-owned. They come from authoritative project/task state, the active lease and policy decisions, and the applicable product-owned contracts. They apply to every supported adapter and are delivered before a task lease.
2. **Workspace instructions** are guidance found in the selected target workspace (for example, that workspace's `AGENTS.md` or any other instruction file the workspace ships). They are scoped to that workspace and task; they may refine how work is performed but cannot override Mayasaba policy, grant capabilities or expand the lease scope.

The controller MUST deliver the same applicable runtime policy meaning and task context to Hermes Agent CLI, Kilo Code CLI and OpenCode CLI. Use each CLI's supported structured input or instruction mechanism; never depend on native file discovery to enforce Mayasaba policy. Where a CLI would otherwise discover instruction files outside the authorized workspace — as OpenCode does with `~/.claude/CLAUDE.md` and `.claude/skills` — the adapter must disable that discovery explicitly.

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
- ACP entry point: `hermes acp` / `hermes-acp`
- ACP uses stdio JSON-RPC
- adapter must treat ACP and stream-json as distinct native transports and normalize both to MCF-v2.

Hermes documents both stream-json CLI output and ACP stdio operation. Source: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/reference/cli-commands.md

### Kilo Code CLI

- executable: `kilo`
- autonomous entry point: `kilo run --auto "<message>"`
- structured output: `kilo run --format json`
- ACP entry point: `kilo acp`
- ACP can use stdio/default local operation; the CLI also exposes host/port options
- `kilo run` supports session continuation and working-directory selection
- cloud commands exist in Kilo, but Mayasaba MUST NOT use `kilo cloud`; only local CLI/ACP execution is supported
- permission behavior in autonomous mode is governed by Kilo's auto-approval configuration; Mayasaba still enforces its own task/workspace/policy gates.

Kilo documents `kilo run --auto`, JSON output, ACP, session controls and cwd options. Source: https://kilo.ai/docs/code-with-ai/platforms/cli

### OpenCode CLI

- executable: `opencode`
- non-interactive entry point: `opencode run "<message>"`
- structured output: `--format json`, which emits a raw JSON **event stream**; `--format` accepts only `default` or `json` for `run`
- because the output is a typed event sequence rather than a single fixed-shape result document, the adapter must normalize the event sequence and must not assume a single terminal object
- working directory: `--dir <path>`
- session resume: `--session <session-id>` (or `--continue`); `--fork` forks rather than continuing in place
- agent selection: `--agent <name>`, where an agent's frontmatter permission map denies anything not explicitly allowed
- autonomous approval: `--auto` approves permissions that are not explicitly denied; Mayasaba MUST NOT pass `--auto` for tasks requiring controller-mediated execution, because approval would bypass the controller gate
- ACP entry point: `opencode acp --cwd <path>`; ACP is newline-delimited JSON-RPC over stdio, protocol version 1, and the process serves multiple sessions until stdin closes
- global flags: `--version`, `--print-logs`, and `--pure` (run without external plugins)
- OpenCode ships server/web/attach/share/github subcommands; Mayasaba MUST NOT use them, because they imply a network service or external side effect. Only local `run` and local `acp` are supported, consistent with the local-only constraint.
- by default OpenCode ingests `~/.claude/CLAUDE.md`, `.claude` prompt content and `.claude/skills`. The adapter MUST set `OPENCODE_DISABLE_CLAUDE_CODE=1` so that files outside Mayasaba-authorized scope can never act as an instruction source. `OPENCODE_DISABLE_DEFAULT_PLUGINS=1` and `OPENCODE_DISABLE_AUTOUPDATE=1` are likewise required to keep adapter behavior deterministic and offline.
- provider credentials live in `~/.local/share/opencode/auth.json`; `opencode auth list` is a readiness probe only. Credentials are external to MCF-v2 and must never be copied into protocol payloads.

OpenCode documents `opencode run`, `--format json`, `--dir`, `--session`, `--agent`, `--auto`, `opencode acp`, global flags and the `OPENCODE_*` environment variables. Source: https://opencode.ai/docs/cli/ and https://opencode.ai/docs/acp/

A `stream-json` output mode for `opencode run` appears in an unmerged third-party pull request and is NOT documented as available. The adapter must not depend on it, and runtime probe remains authoritative for the installed version.

## Adapter transport decision

| Agent | Native automation transport | Session model | Adapter strategy |
|---|---|---|---|
| Hermes | stream-JSON stdio or ACP stdio | Hermes session ID | dedicated stream-JSON adapter; ACP optional capability |
| Kilo | JSON stdio or ACP | Kilo session ID | dedicated JSON adapter; ACP optional capability |
| OpenCode | JSON event stream stdio or ACP stdio | OpenCode session ID | dedicated JSON event-stream adapter; ACP optional capability |

Mayasaba does not force all three agents through ACP. Native structured transports are preferred when they provide the required capabilities; ACP is selected only where the adapter capability matrix confirms it is the correct runtime path.

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

Sources: NousResearch Hermes CLI reference; Kilo Code CLI documentation; OpenCode CLI and ACP documentation.

## Native event normalization

Native CLI output is first normalized to schemas/agent-adapter-v1/native-event.schema.json, then translated according to native-to-mcf.registry.json. Runtime probe results remain authoritative for installed-version behavior.
