# Mayasaba Agent Integration

## Supported agents

Initial support is exactly:

1. Claude Code CLI
2. Hermes Agent CLI
3. Kilo Code CLI
4. Cline

Future agents require an explicit architecture/protocol extension and are not silently added.

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

### Claude Code CLI

- executable: `claude`
- primary automation entry point: `claude -p`
- structured output: `--output-format json` or `--output-format stream-json`
- structured input: `--input-format stream-json` with `-p` and stream-json output
- session continuation: `--resume <session-id>` / `--continue`
- working-directory control is available through the CLI environment/invocation model; adapter must launch with the leased workspace as cwd
- permission controls include `--allowedTools`, `--disallowedTools`, and permission mode flags
- native stream transport: JSONL/stream-JSON over stdio
- exit status remains an OS process signal; final structured `result` is parsed separately from process exit
- authentication is external to MCF-v2 and must never be copied into protocol payloads.

Anthropic documents `claude -p`, JSON/stream-JSON, stream-JSON input, session resume and tool permission flags. Source: https://docs.anthropic.com/en/docs/claude-code/cli-usage

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

### Cline CLI

- executable: `cline`
- one-shot/headless entry point: `cline "<prompt>"`
- structured output: `--json` (NDJSON)
- working directory: `--cwd <path>`
- session resume: `--id <session-id>`
- autonomous approval: `--auto-approve true` / `--yolo`
- ACP entry point: `cline --acp`
- ACP is stdio based
- adapter must keep Cline JSON/headless and ACP as separate native transport implementations.

Cline documents headless JSON/NDJSON, cwd, session IDs, autonomous approval and ACP. Source: https://github.com/cline/cline/blob/main/docs/cli/cli-reference.mdx

## Adapter transport decision

| Agent | Native automation transport | Session model | Adapter strategy |
|---|---|---|---|
| Claude Code | stream-JSON stdio | Claude session ID | dedicated stream-JSON adapter |
| Hermes | stream-JSON stdio or ACP stdio | Hermes session ID | dedicated stream-JSON adapter; ACP optional capability |
| Kilo | JSON stdio or ACP | Kilo session ID | dedicated JSON adapter; ACP optional capability |
| Cline | NDJSON stdio or ACP | Cline session ID | dedicated JSON adapter; ACP optional capability |

Mayasaba does not force all four agents through ACP. Native structured transports are preferred when they provide the required capabilities; ACP is selected only where the adapter capability matrix confirms it is the correct runtime path.

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

The current documented native automation surfaces are runtime facts and must be recorded in the probe result. Claude Code supports `claude -p` with JSON/stream-JSON output and stream-JSON input; Hermes supports `hermes chat -q` with `--format stream-json`; Kilo supports `kilo run` with `--format json` and `--auto`; Cline supports `cline <prompt>` with `--json`, `--cwd`, session IDs and ACP via `--acp`. Runtime probe remains authoritative for the installed version.

Sources: Anthropic Claude Code CLI reference; NousResearch Hermes CLI reference; Kilo Code CLI documentation; Cline CLI reference.

## Native event normalization

Native CLI output is first normalized to schemas/agent-adapter-v1/native-event.schema.json, then translated according to native-to-mcf.registry.json. Runtime probe results remain authoritative for installed-version behavior.
