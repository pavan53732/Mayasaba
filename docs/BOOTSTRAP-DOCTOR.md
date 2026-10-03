# Mayasaba Windows Preflight and Agent Doctor

## Purpose

Before any project work begins, Mayasaba performs deterministic local preflight.

## Product checks

- Windows OS
- writable application data location
- SQLite availability
- project path accessibility
- required local permissions

## Agent checks

For each of Hermes Agent CLI, Kilo Code CLI and OpenCode CLI:

1. detect executable
2. resolve actual executable path
3. determine version
4. verify invocation
5. inspect authentication/readiness
6. detect working-directory support
7. detect structured transport/features
8. detect capabilities
9. run health check
10. create an isolated session only after checks pass

Mayasaba must not rely on fixed install paths.

## Capability discovery

Runtime capability facts may include:

- structured I/O
- streaming
- interrupt
- resume
- autonomous execution
- scoped file modification
- shell (reported capability only; policy and ExecutionService control admission)
- read-only public-web retrieval for user-requested research, if supported
- scoped UI automation for the selected local target app during software validation, if supported
- Git
- patching
- artifact/evidence reporting
- structured errors

Capabilities describe detected CLI behavior, not permission grants. User-requested public-web access remains read-only; email/messages, public posting/form submission, purchases, account changes and general unrelated-app control are never authorized by a probe result. Scoped UI automation may validate the selected local target app for a software task when it causes no external side effects.

## Agent readiness

~~~text
DISCOVERED
→ HANDSHAKING
→ CAPABILITY_VALIDATING
→ WORKSPACE_VALIDATING
→ READY
~~~

Only READY agents receive task leases.

## Missing agents

The user may proceed with fewer than three if an agent is unavailable or intentionally disabled. The Control Room must show actual capacity, for example 2/3 ready.

Missing agents must never be represented as silently participating.

## Kilo remote mode

Remote/cloud gateway execution is outside Mayasaba's product boundary. Local execution is required.

Kilo is a fork of OpenCode and carries the same cloud/share surface, so the prohibition is broader than "remote mode": the doctor must confirm the adapter will not use Kilo's `cloud`, `remote`, `serve`, `attach`, `github`, `pr`, `daemon`, `import`, `plugin`, `plug`, `upgrade`, `uninstall`, `mcp`, `console`, `roll-call`, `profile` subcommands, nor the `--share`, `--cloud-fork`, `--attach`, `--mdns`, `--mdns-domain`, `--refresh` flags, and that session sharing is off in config. The authoritative prohibition list is owned by `schemas/agent-adapter-v1/native-transport-contract.json`.

## Workspace preflight

Before an agent session starts:

- project path is resolved
- workspace identity is established
- Git status/worktree capability is detected
- allowed scope is calculated
- protected integration workspace is identified
- policy scope is attached to the session

## Process supervision

Mayasaba must track actual process state and child processes.

A stop request is not considered complete until the process state is verified.

## Doctor output

The Doctor should produce a structured report containing:

- platform
- agent availability
- versions
- paths
- authentication/readiness
- capabilities
- workspace readiness
- policy status
- blockers
- remediation instructions
- timestamp

The report can be displayed in the Control Room and persisted as evidence.

## Machine-readable contract

Doctor output is canonicalized by `schemas/doctor-v1/doctor-report.schema.json` and is persisted as evidence when a report is generated.

The Control Room reads the report through `get_doctor_report` (DiagnosticsService). The report never contains secrets; authentication is represented as readiness/status facts only.

## Adapter probe authority

The detailed native invocation matrix is `schemas/agent-adapter-v1/native-transport-contract.json`. Runtime probe results conform to `schemas/agent-adapter-v1/probe-result.schema.json` and are authoritative for the installed executable/version.
