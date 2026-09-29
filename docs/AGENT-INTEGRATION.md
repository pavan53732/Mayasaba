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
- shell execution
- browser automation
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