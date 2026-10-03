# Mayasaba Agent Subsystem Architecture

This document defines the internal architecture of the Mayasaba agent subsystem.

For repository instructions that every AI coding agent must follow, see AGENTS.md.

For native CLI adapter integration, see AGENT-INTEGRATION.md.

MCF-v2 is the sole canonical communication contract between Mayasaba and agent runtimes.

## Scope

The subsystem owns:
- agent discovery and registry
- runtime capability detection
- agent sessions
- health monitoring
- process supervision
- task/lease binding
- agent selection
- pause/resume/stop
- crash recovery
- replacement and disablement
- adapter isolation

It does not own requirements, architecture decisions, task truth, final validation, or certification.

## Supported agents

Hermes Agent CLI, Kilo Code CLI, and OpenCode CLI. See DEC-029.

## Internal topology

Agent Manager -> Registry / Session Manager / Health Monitor / Capability Registry -> Adapter Factory -> agent-specific adapters -> local CLI process.

## Session lifecycle

DISCOVERED -> HANDSHAKING -> CAPABILITY_VALIDATING -> WORKSPACE_VALIDATING -> READY -> ACTIVE -> PAUSED -> DRAINING -> STOPPED.

Failure path:
READY/ACTIVE -> LOST -> RECONNECTING -> SYNCING -> READY/ACTIVE.

## Communication

Agents never communicate through an uncontrolled direct channel. All agent communication passes through the adapter and MCF-v2 bus.

## Capability model

Capabilities are runtime facts, not documentation assumptions. Examples include structured transport, streaming, interrupt, resume, autonomous execution, scoped file access, shell execution, Git, scoped UI automation of the selected local target app for software validation, read-only retrieval of public web sources for requested research, artifact reporting, structured errors and working-directory support. A probed capability is not itself permission: authenticated interactions with public/external services, external side-effect actions and general control of unrelated applications are outside product scope. Software tasks may still use scoped UI automation to validate the user-selected local target app when it causes no external side effects.

Capability loss stops new work requiring that capability and triggers deterministic recovery/reassignment rules.

## Health

UNKNOWN, STARTING, HEALTHY, DEGRADED, UNRESPONSIVE, LOST, FAILED, DISABLED.

Health and session state are separate.

## Scheduling

An agent is eligible only when enabled, session-ready, capability-compatible, workspace-compatible, policy-allowed and within concurrency limits. Workload/affinity heuristics are secondary.

## Isolation

An agent session has a project-scoped workspace. Git worktrees are preferred. Agents cannot modify another agent's workspace or the protected integration workspace without controller authorization.

## Recovery

On agent failure:
record failure -> reconcile processes -> verify workspace -> reconcile lease -> recover checkpoint/diff -> restart or reassign -> regenerate current ContextPack -> resume only after gates.

## Adapter boundary

Adapters translate native CLI behavior to MCF-v2 and report actual runtime capabilities. They must not redefine Mayasaba domain semantics.

Detailed native integration is specified in AGENT-INTEGRATION.md.


Execution mechanics are defined in `EXECUTION-KERNEL-DESIGN.md`.

Workspace and integration mechanics are defined in `WORKSPACE-INTEGRATION-DESIGN.md`.

## Material-action binding

Every material action is bound to project + agent + session + workspace + task lease + context snapshot + project epoch + policy scope + required capabilities + capability snapshot. The machine-readable envelope authorization context is defined in schemas/mcf-v2/envelope.schema.json.

The controller validates the binding before a material action is admitted. An envelope is not an authority grant; durable controller state is authoritative.
