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
- **OpenCode** ingests `~/.claude/CLAUDE.md`, `.claude` prompt content and `.claude/skills` — suppress with `OPENCODE_DISABLE_CLAUDE_CODE=1` on **1.x**. That variable does not exist in the 2.x binary, which still references `.claude`; on 2.x the adapter must enforce the boundary itself (see the OpenCode section).

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
- The adapter MUST NOT pass `--yolo` or `--accept-hooks`; both bypass approval prompts and would contradict controller-mediated execution. **The flags are not the only spellings of the bypass, so blocking them alone is insufficient.** `--accept-hooks` documents itself as "Equivalent to `HERMES_ACCEPT_HOOKS=1` or `hooks_auto_accept: true` in config.yaml", and `--yolo` sets the `HERMES_YOLO_MODE` environment variable. A child process that inherits `HERMES_YOLO_MODE=1` or `HERMES_ACCEPT_HOOKS=1`, or that runs against a user config carrying `approvals.mode: off` (documented as equivalent to `--yolo`), reaches the same approval bypass while passing neither flag. The adapter MUST therefore also scrub both variables from the child environment and MUST neutralize a user config that disables approvals (e.g. via `--ignore-user-config`, or by verifying the resolved approvals mode) before admitting any task requiring execution mediation. A `-q` run has no TTY to answer an approval prompt, so it depends on the fail-closed single-query approvals path, which must not be disabled.
- Hermes exposes outbound-messaging, service, credential and import subcommands (`send`, `slack`, `whatsapp`, `whatsapp-cloud`, `webhook`, `peer`, `gateway`, `portal`, `egress`, `proxy`, `cron`, `kanban`, `sync`, `serve`, `dashboard`, `desktop`, `computer-use`, `browser`, `vault`, `secrets`, `claw`, `import-agent`, `codex-runtime`, `monitoring`, `pairing`). Mayasaba MUST NOT invoke them. Hermes `debug share` also uploads system info and logs to a paste service and offers a `--no-redact` flag; the adapter MUST NOT invoke it.

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
- Kilo is a **fork of OpenCode** and carries the same cloud/share surface. Mayasaba MUST NOT use Kilo's cloud/remote surface: `cloud`, `remote`, `serve`, `attach`, `github`, `pr`, `daemon`, `import`, `plugin` (alias `plug`), `upgrade`, `uninstall`, `mcp`, `console`, `roll-call`, `profile`, nor the `--share`, `--cloud-fork`, `--attach`, `--mdns`, `--mdns-domain` or `--refresh` flags. `--share` uploads the session to a public share URL, `--cloud-fork` fetches a session from cloud, and `--attach` connects to a running kilo server. Only local `run` and local `acp` execution is supported. The install/removal commands (`plugin`, `upgrade`, `uninstall`) are code installation or removal over the network, which DEC-024 requires to be controller-mediated and DEC-004 confines to local execution; `KILO_DISABLE_AUTOUPDATE=1` closes only the automatic update path, not the explicit `upgrade` subcommand, and `KILO_DISABLE_DEFAULT_PLUGINS=1` disables default plugins only, not the `plugin` installer. `mcp add --url` registers a remote MCP server and `mcp auth` performs OAuth against it (DEC-028); `mcp add --env` spawns a local MCP server process (DEC-024). `roll-call` and `profile` reach model providers and the Kilo gateway. `console` opens or stops a local HTTP service, the same class as the already-forbidden `serve`/`daemon`. **`--mdns` is a global flag, so it is reachable on the admitted `acp` vector** — it defaults the listen hostname to `0.0.0.0` and advertises the service on the LAN, so the admitted `acp` invocation must bind `127.0.0.1` and must never pass `--mdns`. Verified against the installed binary (kilo 7.8.1).
- **session sharing has three independent triggers and the adapter must close all three.** The share step runs unless `config.share !== "auto" && !autoShare && !args.share`, so a session is uploaded when the config key `share` is `"auto"`, **or** the `KILO_AUTO_SHARE` env var is truthy, **or** the `--share` flag is passed. Blocking the flag alone is not sufficient. The adapter MUST set `share: "disabled"` in config — a **string** enum (`"manual"` default, `"auto"`, `"disabled"`), not a boolean — and MUST set `KILO_AUTO_SHARE=0`. Kilo also exposes its own hard kill switch, `KILO_DISABLE_SHARE=1`, which the adapter MUST set.
- because Kilo is an OpenCode fork, it ingests `~/.claude/CLAUDE.md`, `~/.claude/skills` and `~/.claude.json` by default — paths outside Mayasaba-authorized scope. The adapter MUST set the following seven `KILO_DISABLE_*` variables in one place: `KILO_DISABLE_CLAUDE_CODE=1`, `KILO_DISABLE_CLAUDE_CODE_SKILLS=1`, `KILO_DISABLE_CLAUDE_CODE_PROMPT=1`, `KILO_DISABLE_DEFAULT_PLUGINS=1`, `KILO_DISABLE_AUTOUPDATE=1`, `KILO_DISABLE_SHARE=1` and `KILO_DISABLE_CODEBASE_INDEXING=1`. Kilo reads the `KILO_`-prefixed variables; `OPENCODE_DISABLE_CLAUDE_CODE` has no effect on the Kilo binary and must not be relied on.
- **codebase indexing is a second, independent egress path.** Kilo enables codebase indexing by default and uploads code embeddings to a configured vector store; on the reference machine that was a remote qdrant endpoint with a provider API key. This is code egress outside Mayasaba-authorized scope, and disabling session sharing does not close it. The adapter MUST set `KILO_DISABLE_CODEBASE_INDEXING=1`.
- **the resolved permission map is observable, and the adapter must prove it.** `kilo debug config` emits the resolved configuration as JSON, including a `permission_origins` object, and `kilo debug agent <name>` emits that agent's resolved permission array. The adapter must inject its restrictive map (see below) and then verify from these probes that the denies resolved at origin `local` rather than `global`. **The probe must never pass `--tool`:** `kilo debug agent <name> --tool <id> --params <json>` EXECUTES the named tool rather than reporting a permission decision, so using it to "test" whether an operation is permitted would itself perform unmediated execution during admission.
- **permission enforcement (hard admission precondition, enforceable):** `--auto` is retained in the launch vector, but the adapter MUST fail admission for any task requiring execution mediation unless it first **injects** the restrictive map in `required_config_injection` via `KILO_CONFIG_CONTENT` and then proves from the probe that the resolved map matches. The map is **default-deny with an explicit allow-list** — `{"*": "deny"}` is the base rule and only `read`, `glob`, `grep`, `edit`, `write` and `semantic_search` are re-allowed — **not** a list of specific denies. A specific-deny list is insufficient because several privileged tools are governed by **no named permission key at all** — only by the wildcard `*`. `cron_create`, `schedule_wakeup`, `task`, `board_post`, `link_pr` and `todowrite` resolve to `allow` under the global config and are not closed by denying `bash`, `external_directory`, `webfetch` or `websearch`. (Tools that *do* name a key are correctly blocked by a targeted deny: `background_process` — an arbitrary shell command with a caller-chosen `cwd` — asks under the `bash` and `external_directory` keys, so `bash: "deny"` does stop it; verified end-to-end that a `background_process` start is refused and no process is spawned. It is the wildcard-only tools that a specific-deny list misses.) Kilo's own built-in restricted profile uses the same `{"*": "deny"}` base, so this is the vendor's own shape, not an invention. `KILO_CONFIG_CONTENT` is a documented Kilo config channel read at the same `Config.Service` the `run` path uses; it resolves at origin `local` and outranks the user's global config. Inside a pattern map the **last** matching pattern wins (`findLast`), so the nested `read` map must list `"*": "allow"` **before** `"*.env": "deny"` and `"*.env.*": "deny"` — reversing that order silently re-opens Kilo's own secret-file read guard. `--auto` auto-approves anything that resolves to `ask`, so a residual `ask` is an approval rather than a prompt; the default-deny base converts those to `deny`, which `Permission.ask` treats as terminal before the auto-approve loop runs (DEC-024). Verify from `kilo debug config` (resolved config + `permission_origins`) and `kilo debug agent <name>` (that agent's resolved permission array) that the injected rules resolved at origin `local` and that the enabled-tool set is exactly the six expected. Do not assume Kilo's defaults are restrictive: on the reference machine the user's global config set `bash`, `external_directory`, `webfetch` and `websearch` to `allow`.

Kilo documents `kilo run`, JSON output, ACP, session controls and cwd options. Source: https://kilo.ai/docs/code-with-ai/platforms/cli

**Correction (overstated attribution):** an earlier revision of this document attributed Kilo's cloud prohibition to a specific named command only (`kilo cloud`). The prohibition is broader — see the list above — and the fork relationship to OpenCode is why Kilo's prohibited surface is kept aligned with OpenCode's.

### OpenCode CLI

- executable: `opencode`
- non-interactive entry point: `opencode run "<message>"`
- structured output: `--format json`, which emits a raw JSON **event stream**; `--format` accepts only `default` or `json` for `run`
- because the output is a typed event sequence rather than a single fixed-shape result document, the adapter must normalize the event sequence and must not assume a single terminal object
- **completion semantics:** the process exit code is non-authoritative. The adapter MUST treat exit-0-with-empty-output as a failure, derive completion from observed event content rather than a terminator record, and preserve subagent-origin parts explicitly because parts are filtered to the root session.
- version gate: the **1.x** line is admitted and accepts `--dir <path>`; an absolute path is resolved directly and is the form the adapter must use. The **2.x** line is unverified. **Correction:** an earlier revision said 2.x "is not published to npm" — that checked only the `opencode-ai` package. 2.x **is** published, as `@opencode/cli` (latest `2.0.22`; the line runs 2.0.0–2.0.22 contiguously), while `opencode-ai` carries only majors 0 and 1 with latest `1.18.34`. Both are the same project (`@opencode/cli` declares `anomalyco/opencode`; `sst/opencode` 301-redirects there). The correction matters in the unsafe direction: 2.x is an installable artifact, so the gate is a live control. The adapter must branch on the probed version and must fail closed on a version it cannot classify.
- **every vector in this section is 1.x-shaped, not just the workspace flag.** On 2.x `run --dir`, `acp --cwd` and `run --pure` are each rejected as an unrecognized flag (verified against the installed 2.0.22 binary: `--dir`, `--pure` and `--share` occur zero times in it). An adapter that selects a vector by transport name rather than by probed line fails at launch on 2.x. The adapter must key launch, resume, ACP and determinism vectors to the probed line.
- **on 2.x there is no workspace flag at all.** `--dir` is not merely rejected: no `dir` flag exists on `run` or on any other 2.x command. The root-level `directory` positional is not a substitute — `run`'s trailing arguments are the variadic `message...`, so a path passed there is consumed as prompt text (verified: `run --format json hi <dir>` bound the session to the child cwd, not `<dir>`). The workspace must be supplied as the child process working directory.
- **working-directory hazard (2.x).** The 2.x `run` path resolves its root as `options.root ?? process.env.PWD ?? process.cwd()` and then **changes directory into it** (`packages/cli/src/run/run.ts:73-74` calls `localDirectory(root)`, which at lines 167-172 performs `process.chdir(root)`). A spawned child that inherits a stale `PWD` therefore chdirs into the wrong directory, and `run` offers no flag to override it. **`PWD` does not merely participate — it overrides the child working directory**, so setting the cwd is not sufficient on its own. Verified by execution against the installed 2.0.22 Windows binary, with a project config defining an agent in one directory only, testing both discordant cells: `cwd=<probe>, PWD=<other>` → `Agent not found`; `cwd=<other>, PWD=<probe>` → the agent resolved and ran. The outcome tracked `PWD` in both cells. The adapter MUST therefore set or clear `PWD` in the child environment; clearing it is acceptable because `process.cwd()` is then used and the cwd is correct. This is a scope-integrity requirement: an inherited `PWD` can point the agent outside the authorized workspace. On Windows `PWD` is normally absent, so the default path is correct — but MSYS/git-bash parents export it, which is exactly how a stale value is inherited. **Verifier note:** the `debug` subcommands (`debug config`, `debug agents`) resolve configuration *without* going through `run`'s chdir path, so they cannot be used to probe this hazard; a probe built on them returns a false negative and wrongly concludes `PWD` is ignored. Only `run` exposes it. (An earlier revision said the run path "never changes directory"; that was read from the thin command handler `packages/cli/src/commands/handlers/run.ts`, which only delegates to `runNonInteractive`, not from the run path itself. Corrected against the `v2.0.22` tag.)
- session resume: `--session <session-id>` (or `--continue`); `--fork` forks rather than continuing in place
- agent selection: `--agent <name>`, where an agent's permission map denies anything not explicitly allowed. **Verified on 2.0.22**: a config-defined agent's permission map is applied (an agent declaring `bash`/`edit` deny resolves to those denies after the inherited `{"*":"allow"}` base), an undefined agent fails closed (`Agent not found`), and the built-in `explore` agent demonstrates a default-deny base followed by targeted re-allows — so last-match-wins ordering applies, as it does for Kilo. **But permission action names are not validated**: `bash`→`shell`, `write`/`patch`→`edit`, `task`→`subagent`, and an unknown name is passed through silently rather than rejected, so a typo fails **open**. The adapter must confirm the resolved map via `debug agents` rather than trust its own input. 2.x exposes no working inline config-injection channel (unlike Kilo's `KILO_CONFIG_CONTENT`), so the restricted agent must be installed through a config file the adapter controls.
- autonomous approval: `--auto` approves permissions that are not explicitly denied; Mayasaba MUST NOT pass `--auto` for tasks requiring controller-mediated execution, because approval would bypass the controller gate. The 2.x binary carries `--auto` with the same wording, so the prohibition holds on both lines.
- ACP entry point (1.x only): `opencode acp --cwd <path>`; ACP is newline-delimited JSON-RPC over stdio, protocol version 1, and the process serves multiple sessions until stdin closes. **On 2.x the `acp` command accepts no flags beyond the globals** — `--cwd` is rejected as an unrecognized flag — so the 2.x ACP workspace must come from the spawned child's cwd (or the controller-supplied `cwd` in the ACP `session/new` request).
- global flags (1.x only): `--version`, `--print-logs`, and `--pure` (run without external plugins). `--pure` does not exist on 2.x (rejected as an unrecognized flag; zero occurrences in the binary), and there is no equivalent; determinism on that line comes from the required environment and config only.
- OpenCode ships `serve`, `web`, `attach`, `github`, `pr` and `import` subcommands; Mayasaba MUST NOT use them, because they start network services or reach external endpoints. `pr <number>` fetches and checks out a GitHub PR branch, and `import <file>` accepts a share URL as well as a local JSON path — both perform outbound network access. Only local `run` and local `acp` are supported, consistent with the local-only constraint. Note that OpenCode 1.x registers the same broader surface as its fork (plugin, upgrade, uninstall, mcp, console, models); the adapter must not rely on the fork's longer prohibition list to constrain the upstream binary. Verified against the 1.x source tree at tag `v1.18.34`.
- **the 2.x remote surface is different and must be forbidden separately.** Verified against the installed 2.0.22 binary, `web`, `attach`, `github`, `pr` and `import` are **not** 2.x subcommands — they fall through to the root default handler (proved by an identical-output control against a nonexistent command name). The 2.x network/server surface is instead `serve` (`--hostname`/`--port`/`--cors`), `service` (start/restart/stop/status/get/set/unset the background server), `pair` (prints one-time connect links; `--url` embeds an arbitrary external URL in a live credential link), `api` (HTTP requests to the running server; `--server`/`--data`/`--header`), plus `mcp`, `plugin`, `upgrade`, `uninstall`, `models`, `reload`, `mini` and `stats`. `--server <url>` is reachable on the **admitted** `run` and `acp` vectors and connects to an arbitrary URL (verified), so it is forbidden. `--standalone` is **required**, not forbidden: it gives each admission a private loopback server instead of the shared background service. **`stats` is 2.x-only and is the one entry whose hazard is disclosure rather than egress:** run with no `--project` filter it aggregates **every** project in the machine-wide database, not only the authorized workspace — verified against the installed binary, where unfiltered `stats --json` reported 105 sessions across all 42 projects while `stats --project .` reported 21 for the current workspace. It is the same class as `session export`: a read path that is not workspace-confined.
- **on 2.x an unrecognised leading token is swallowed as the root `directory` positional, not rejected.** `opencode web` and `opencode zzznotreal` produce the **identical** error — `Error: ENOENT: no such file or directory, chdir '<cwd>' -> 'web'` / `-> 'zzznotreal'` — and with a real directory of that name present the token chdirs into it and the invocation proceeds (verified against the installed 2.0.22 binary). So "the CLI accepted the argv" is **not** evidence a token was a valid subcommand, and a name-based blocklist cannot tell a forbidden command from a path the user meant. The adapter must validate every argv element against the probed line's admitted surface itself; the union forbidden list is retained rather than narrowed to the probed line precisely because an over-broad list fails closed.
- **ACP is itself a scope-extension channel on 2.x.** The `initialize` handshake advertises `sessionCapabilities.additionalDirectories`, and `session/new` accepts an `additionalDirectories` array that is honored — a session was created with `additionalDirectories: ["C:/Windows/Temp"]`, outside the authorized workspace, and succeeded (verified). This bypasses the launch vector entirely, so the adapter must be the **sole** author of ACP params and must reject or clamp `additionalDirectories` to the authorized workspace. Correspondingly, the workspace on the ACP path is bound by the **required** `session/new` `cwd` field — omitting it fails closed (`-32602`), it is honored, and it **overrides** the process cwd, which also neutralizes the stale-`PWD` hazard for that path. `session/request_permission` makes the ACP client the approval authority, so the adapter must route permission decisions through the controller and never auto-approve (DEC-024).
- **forbidding a top-level name is not sufficient on 2.x.** Sub-subcommands reach credentials and external URLs under otherwise benign parents: `auth export` prints stored credentials *including secrets* and `auth import` consumes them; `session import <file>` accepts a **URL**; `mcp add`/`mcp auth` register and authenticate a remote MCP server; `plugin add`/`update` install code over the network (DEC-024 controller-mediated). The prohibition is therefore at the subcommand level while `auth list` remains the readiness probe and `mcp list`/`plugin list` stay read-only. `console` does not exist on 2.0.22. `debug` is **not** forbidden, deliberately: `debug agents` and `debug config` are the verification probes this contract *requires* the adapter to run, and `debug paths` resolves the global paths — but the adapter must treat `debug paths` output as machine-local path material (it names user-home, config, state and database locations outside the workspace) and must not surface it in MCF payloads, evidence or the UI.
- **2.x defaults to a persistent background HTTP service shared across invocations** (`service status` reports a live `127.0.0.1` listener). It is loopback-bound, so DEC-004 holds, but state is not isolated per invocation; the adapter MUST run 2.x with `--standalone`.
- **two further 2.x scope hazards.** (1) Project-config discovery **walks up from the child working directory** into its ancestors: an `opencode.json` in a parent of the authorized workspace — including one outside it — contributes configuration (verified). (2) The binary subscribes file watchers to user-home instruction/skill directories outside any workspace (`~/.claude/skills`, `~/.agents/skills`; observed in its own debug log), and on 2.x no environment variable disables this, because `OPENCODE_DISABLE_CLAUDE_CODE` does not exist on that line. The adapter must enforce both boundaries itself rather than rely on discovery.
- **session sharing has three independent triggers and the adapter must close all three.** `share` is **not** a subcommand — it is a boolean flag on `run`. The share step runs unless `config.share !== "auto" && !autoShare && !args.share`, so a session is uploaded when the config key `share` is `"auto"`, **or** the `OPENCODE_AUTO_SHARE` env var is truthy, **or** the `--share` flag is passed. Blocking the token `share` as a subcommand would not prevent `opencode run --share`. The adapter MUST block `--share` as a flag, set `OPENCODE_AUTO_SHARE=0`, require `share: "disabled"` in config — a **string** enum (`"manual"` default, `"auto"`, `"disabled"`), not a boolean — and set `OPENCODE_DISABLE_SHARE=1`, OpenCode's own hard kill switch for the share subsystem (verified at `packages/opencode/src/share/share-next.ts:23`, truthy on `"true"`/`"1"`, gating the upload path at six call sites).
- **the five required environment variables are 1.x-scoped.** Verified against the installed 2.0.22 binary, only `OPENCODE_DISABLE_AUTOUPDATE` survives there; `OPENCODE_DISABLE_CLAUDE_CODE`, `OPENCODE_DISABLE_DEFAULT_PLUGINS`, `OPENCODE_AUTO_SHARE` and `OPENCODE_DISABLE_SHARE` occur **zero** times in it, so setting them on 2.x is a no-op rather than a control. On 1.x the adapter MUST set all five: `OPENCODE_DISABLE_CLAUDE_CODE=1` stops OpenCode ingesting `~/.claude/CLAUDE.md`, `.claude` prompt content and `.claude/skills` (paths outside authorized scope that must never act as an instruction source), and the others keep behavior deterministic, offline and non-uploading. On 2.x, where `OPENCODE_DISABLE_CLAUDE_CODE` is absent and the binary still references `.claude`, the instruction-source boundary must be enforced by the adapter.
- **2.x DOES have an inline config-injection channel: `OPENCODE_CONFIG_CONTENT`, and it is the controller's mechanism for installing policy.** An earlier revision of this document asserted the opposite and told the adapter to write a restricted-agent config *file* into the workspace or config directory. That was wrong on both counts. Verified against the installed 2.0.22 binary: `OPENCODE_CONFIG_CONTENT` is a first-class **virtual** config source (no path) carrying the injected keys, and its **precedence is highest** — with a project `opencode.json` also in scope, the resolved source order is `[0]` user config directory, `[1]` project document, `[2]` the env-content document, applied last so it wins. It is also strictly better than the file approach: it writes no policy artifact into user-controlled territory and does not interact with the ancestor-config hazard. The adapter MUST install `share: "disabled"` and the restricted agent permission map through this channel on 2.x.
- **the config-injection probe must not use `debug config`/`debug agents`.** Those subcommands are answered by the **shared background service** and ignore the invoking process's environment — verified: with `OPENCODE_CONFIG_CONTENT` set, `debug config` did not show the injected source while a private `serve` read over its API did. So the method this document previously required ("verify the resolved map via `debug agents`") is **unsound on 2.x** and can certify a policy the child never applies. The adapter must read resolved configuration from a server it started itself with a caller-chosen credential (`OPENCODE_SERVER_PASSWORD`), not from `debug` against a pre-existing service; `opencode debug --standalone` is not accepted, so there is no standalone escape for the debug path itself.
- **the 2.x share control is config-only and has a second spelling.** On 1.x a session is uploaded when config `share` is `"auto"`, **or** `OPENCODE_AUTO_SHARE` is truthy, **or** `--share` is passed. On 2.x the flag and the env var do not exist, but the config path remains and gains a deprecated `autoshare: true` boolean that normalizes to `share: "auto"` (verified; an explicit `share: "disabled"` overrides it back). Because 2.x has no env or flag kill switch, `share: "disabled"` in config is the **only** load-bearing share control on that line. The subsystem is still present on 2.x (`share` enum, `share_url`, TUI share/unshare, enterprise sharing), so this is not dormant.
- provider credentials live in `~/.local/share/opencode/auth.json`; `opencode auth list` is a readiness probe only. Credentials are external to MCF-v2 and must never be copied into protocol payloads. Two hazards: `auth list` enumerates stored provider credentials by provider/account, so its output is credential material; and `auth export` is documented as printing stored credentials **including secrets**, so it is a credential-exfiltration surface the adapter must never invoke.

OpenCode documents `opencode run`, `--format json`, `--dir`, `--session`, `--agent`, `--auto`, `opencode acp`, global flags and the `OPENCODE_*` environment variables. Source: https://opencode.ai/docs/cli/ and https://opencode.ai/docs/acp/ — but note the documentation describes the **1.x** line; `--dir`, `--pure` and `--share` do not exist on 2.x, and four of the five `OPENCODE_*` variables do not either. The runtime probe of the installed version is authoritative over both this document and upstream documentation.

**Correction (1.x-scoped vectors and a false npm claim):** an earlier revision of this section presented the launch, ACP and determinism vectors, and the five environment variables, as universal, and stated that the 2.x line "is not published to npm as `latest`". Verified against the installed `opencode` v2.0.22 (package `@opencode/cli`): the npm claim checked only the `opencode-ai` package and is false — 2.x is published as `@opencode/cli`, latest 2.0.22. Every vector in the section is 1.x-shaped and invalid on 2.x, not only the workspace flag; four of the five environment variables are absent from the 2.x binary; and the 2.x remote surface, share triggers, background service and config/instruction-discovery boundaries differ from 1.x as recorded above. The version gate now scopes each of these by line, and `tools/contracts/verify.mjs` fails when a line-scoped field or a 2.x scope hazard is deleted.

**Correction (wrong 2.x remediation and an unsound probe):** an earlier revision of this section stated that "2.x exposes no working inline config-injection channel" and instructed the adapter to install its restricted agent through a config **file** "within the workspace or the resolved config directory". Both parts were wrong. Verified against the installed 2.0.22 binary, `OPENCODE_CONFIG_CONTENT` **is** that channel, it is a virtual (pathless) source, and its precedence is **highest** — applied after the user config directory and the project document. The file approach it replaced was also the worse option: it writes a policy artifact into user-controlled territory and interacts with the ancestor-config hazard recorded above. The root cause of the error was the probe: `debug config`/`debug agents` are answered by the shared background service and ignore the invoking process's environment, so they return a false negative for any env-injected source, and an admission gate built on them can certify a policy the child never applies. This section, `permission_model_2x`, `permission_enforcement` and the forbidden-subcommand rationale no longer recommend them as the verification probe; the contract requires reading resolved configuration from a private server the adapter starts itself. A follow-on adversarial pass over the installed binary also added `stats` (a cross-workspace read: unfiltered it aggregated 105 sessions across all 42 projects) to the forbidden list, recorded that an unrecognised leading token is swallowed as the root `directory` positional rather than rejected, and recorded that ACP's `additionalDirectories` is a workspace-scope extension channel.

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
