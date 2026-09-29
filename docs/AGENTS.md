# Mayasaba Agent Architecture

## 1. Purpose

This document defines the complete internal agent subsystem of Mayasaba.

It complements AGENT-INTEGRATION.md:

- AGENT-INTEGRATION.md defines the adapter contract and native CLI integration boundary.
- AGENTS.md defines the internal agent manager, registry, session lifecycle, capability model, scheduling interaction, health, isolation, recovery and four supported agents.

MCF-v2 remains the only canonical communication contract between Mayasaba and agent runtimes.

## 2. Initial supported agents

The initial supported set is exactly:

1. Claude Code CLI
2. Hermes Agent CLI
3. Kilo Code CLI
4. Cline

No additional agent is silently included.

The architecture is extensible through adapters, but adding another agent requires an explicit capability/adapter contract and implementation decision.

## 3. Agent architecture

~~~text
                    AGENT MANAGER
                         |
        +----------------+----------------+
        |                |                |
   Agent Registry   Session Manager   Health Monitor
        |                |                |
        +----------------+----------------+
                         |
                  Capability Registry
                         |
                  Adapter Factory
                         |
       +---------+---------+---------+---------+
       |         |         |         |         |
     Claude    Hermes    Kilo      Cline    Future
    Adapter   Adapter   Adapter   Adapter   Adapter
       |         |         |         |
       +---------+---------+---------+---------+
                         |
                   Local CLI Process
                         |
                       Agent
~~~

## 4. Agent Manager responsibilities

The Agent Manager is responsible for:

- discovering supported agents
- registering runtime instances
- creating sessions
- negotiating capabilities
- assigning session identity
- supervising lifecycle
- exposing health state
- routing MCF-v2 messages through adapters
- enforcing project/session/workspace binding
- coordinating pause/resume/stop
- recovering failed sessions
- reporting capability loss
- preventing unauthorized cross-project use

The Agent Manager does not own:

- project requirements
- architecture decisions
- task truth
- final validation
- certification

Those remain with their canonical subsystems.

## 5. Agent Registry

Each registered agent has:

- agent_id
- agent_type
- display_name
- executable/path reference
- detected version
- adapter version
- installation state
- authentication/readiness state
- capability set
- health state
- process state
- supported transport
- discovery timestamp
- last health check
- configuration reference

The registry is runtime-derived and must not assume fixed installation paths.

## 6. Agent identity

Agent identity is separate from process identity.

~~~text
Agent Type
   +
Registered Agent Instance
   +
Agent Session
   +
OS Process
~~~

A process restart creates a new process identity while the logical agent/session may be recovered or replaced according to recovery rules.

Every active session is bound to:

- project_id
- agent_id
- session_id
- adapter
- workspace_id
- task lease when working
- context snapshot
- project epoch
- policy scope

## 7. Agent session lifecycle

Canonical session state:

~~~text
DISCOVERED
  ↓
HANDSHAKING
  ↓
CAPABILITY_VALIDATING
  ↓
WORKSPACE_VALIDATING
  ↓
READY
  ↓
ACTIVE
  ↓
PAUSED
  ↓
DRAINING
  ↓
STOPPED
~~~

Failure/recovery:

~~~text
READY / ACTIVE
      ↓
     LOST
      ↓
RECONNECTING
      ↓
   SYNCING
      ↓
READY / ACTIVE
~~~

Terminal failure:

~~~text
RECONNECTING → FAILED
~~~

A task lease cannot be issued to a session that is not READY or ACTIVE as permitted by the task operation.

## 8. Handshake

Before an agent becomes READY:

1. launch adapter
2. establish local process/session
3. verify CLI identity
4. negotiate protocol transport
5. validate supported MCF-v2 version
6. obtain runtime capabilities
7. verify working directory
8. verify project workspace
9. verify policy scope
10. verify process health
11. persist session
12. emit READY

A documentation claim is never sufficient to mark a capability as available.

## 9. Capability model

Capabilities are typed runtime facts.

Examples:

- structured_transport
- streaming
- interrupt
- resume
- autonomous_execution
- file_read
- file_write
- shell_execution
- git
- patching
- browser
- artifact_reporting
- structured_errors
- working_directory
- non_interactive_execution

Each capability has:

- capability_id
- supported
- version/variant where relevant
- detected_at
- evidence reference
- confidence/status

## 10. Capability requirements

Tasks declare required capabilities.

Scheduling checks:

~~~text
TASK REQUIREMENTS
       ↓
AGENT CAPABILITY SET
       ↓
ALL REQUIRED CAPABILITIES?
   /              \
 YES              NO
 ↓                 ↓
ELIGIBLE       NOT ELIGIBLE
~~~

An unsupported capability cannot be silently substituted.

If an equivalent capability exists through another explicitly supported local mechanism, the orchestrator may use it only when the task/policy contract permits that mechanism.

## 11. Dynamic capability loss

Capabilities can disappear after a session is READY.

Examples:

- CLI transport failure
- browser unavailable
- process loses permission
- tool becomes unavailable
- adapter detects incompatible runtime state

The adapter reports capability loss.

Mayasaba then:

1. stops new work requiring the capability
2. protects the active task
3. evaluates whether current work can safely continue
4. pauses or interrupts affected operations
5. regenerates capability state
6. reassigns or resumes if safe
7. records the event and evidence

## 12. Health model

Agent health states:

- UNKNOWN
- STARTING
- HEALTHY
- DEGRADED
- UNRESPONSIVE
- LOST
- FAILED
- DISABLED

Health is separate from session state.

A healthy CLI can have a paused session.
A degraded CLI may still execute tasks that do not require the missing capability.

## 13. Health monitoring

Health checks observe:

- process existence
- process responsiveness
- adapter transport
- heartbeat/communication
- capability availability
- workspace access
- command responsiveness
- recent error rate

Health checks must not interfere with active work.

## 14. Agent selection

Mayasaba selects an agent using deterministic constraints before any preference heuristic:

1. agent is enabled
2. session is available
3. required capabilities exist
4. project/workspace is compatible
5. policy permits required actions
6. concurrency capacity exists
7. task dependencies are satisfied

Only then may scheduling policy consider:

- current workload
- task affinity
- recent failures
- recovery state
- agent availability

No agent is selected merely because it claimed to be capable.

## 15. Concurrency

Each agent has configurable concurrency limits.

Default principle:

~~~text
one agent session
→ one authoritative active task
→ one isolated workspace
~~~

Additional sessions are allowed only when the adapter/runtime and workspace model explicitly support safe parallelism.

An agent must never receive two leases that can concurrently mutate the same protected workspace.

## 16. Task lease binding

An active implementation task requires:

- task_id
- lease_id
- agent_id
- session_id
- workspace_id
- project_epoch
- context_snapshot_id
- capability requirements
- allowed paths
- expiry
- heartbeat

A session cannot perform material task actions after its lease expires.

## 17. Agent-to-agent communication

Agents do not open direct communication channels to one another.

Instead:

~~~text
Claude
  ↓
Claude Adapter
  ↓
MCF-v2
  ↓
Mayasaba Bus
  ↓
MCF-v2
  ↓
Hermes Adapter
  ↓
Hermes
~~~

The same path applies to Kilo and Cline.

This ensures:

- auditability
- project isolation
- ordering
- ACK/retry
- context validation
- policy enforcement
- causal tracing
- replay

## 18. Agent message authority

Agents can produce:

- proposals
- questions
- critiques
- rebuttals
- implementation reports
- failures
- diagnoses
- repair results
- reviews
- test results
- evidence references

Agents cannot directly produce authoritative:

- project phase transitions
- HARD_LOCK state changes
- task ownership without lease
- integration authority
- validation certification
- final certification

## 19. Four agent adapters

### ClaudeCodeAdapter

Responsibilities:

- detect Claude Code CLI
- determine version
- establish supported local machine interface
- launch process
- map native structured output into MCF-v2
- support streaming where available
- interrupt/stop
- collect changes/evidence
- report runtime capability facts

Native CLI behavior remains adapter-owned.

### HermesAdapter

Responsibilities:

- detect Hermes Agent CLI
- determine version
- establish supported local transport
- launch local session
- map native protocol into MCF-v2
- support structured communication where available
- interrupt/stop/recovery
- collect changes/evidence

### KiloAdapter

Responsibilities:

- detect Kilo Code CLI
- determine version
- establish supported local automation interface
- launch local session
- map native transport into MCF-v2
- support autonomous/local execution where available
- interrupt/stop/recovery
- collect changes/evidence

Remote/cloud gateway execution is outside Mayasaba's product boundary.

### ClineAdapter

Responsibilities:

- detect Cline runtime/CLI interface supported by the installed version
- determine version
- establish supported local automation transport
- launch/supervise session
- map native communication into MCF-v2
- interrupt/stop/recovery where supported
- collect changes/evidence

The adapter must report actual runtime capabilities rather than assuming capabilities from product documentation.

## 20. Adapter isolation

Adapters may depend on:

- MCF-v2 protocol types
- local process primitives
- workspace APIs
- agent-specific configuration

Adapters must not depend on:

- React
- Control Room UI
- project database internals
- unrelated agent adapters
- another agent's private session

## 21. Agent configuration

Agent configuration contains only adapter/runtime settings.

Examples:

- executable/path
- invocation mode
- environment reference
- transport mode
- timeout
- startup timeout
- heartbeat interval
- concurrency limit
- enabled/disabled
- capability overrides only when explicitly supported

Secrets should remain with the owning CLI configuration whenever possible.

Mayasaba stores references rather than copying credentials into ordinary project state.

## 22. Process supervision

The Agent Manager tracks:

- process ID
- parent process
- child processes where observable
- start time
- exit state
- termination reason
- adapter session
- task/lease
- workspace

Stop means verified process termination, not merely sending a signal.

## 23. Pause/resume

Pause semantics:

1. stop assigning new work
2. notify affected sessions
3. allow safe checkpoint/drain where possible
4. interrupt active operations when required by policy
5. persist paused state
6. retain leases only when explicitly safe
7. resume only after session/context/workspace validation

Resume revalidates capabilities and context before material work continues.

## 24. Agent crash recovery

On crash:

~~~text
PROCESS LOST
→ SESSION LOST
→ RECORD FAILURE
→ RECONCILE CHILD PROCESSES
→ VERIFY WORKSPACE
→ RECONCILE LEASE
→ CHECKPOINT/DIFF RECOVERY
→ RESTART OR REASSIGN
→ CREATE CURRENT CONTEXTPACK
→ RESUME ONLY AFTER GATES
~~~

No agent private memory is trusted as authoritative recovery state.

## 25. Handoff interaction

An agent may request handoff.

The handoff must contain:

- current task state
- completed work
- pending work
- changed files
- checkpoint/diff/commit
- tests
- failures
- evidence
- risks
- requirement/decision/contract references
- current context snapshot
- state digest

The receiver must accept the handoff before ownership transfers.

## 26. Agent replacement

An unavailable agent can be replaced only if:

- another enabled agent has required capabilities
- task/workspace can safely transfer
- current context can be regenerated
- lease transition is valid
- policy allows replacement

Replacement is recorded as an explicit event.

## 27. Agent disablement

An agent may be disabled by:

- user configuration
- failed health policy
- incompatible version
- security/policy block
- repeated unrecoverable failures

Disabling prevents new assignments.

Active work follows pause/recovery/reassignment rules.

## 28. Agent observability

The Control Room exposes:

- agent identity
- version
- readiness
- health
- session
- current task
- lease
- workspace
- capability state
- communication status
- process status
- retry/recovery status
- recent failures

It does not expose private chain-of-thought.

## 29. Agent event model

Agent subsystem events include:

- AgentDiscovered
- AgentRegistered
- AgentHandshakeStarted
- AgentReady
- AgentCapabilityChanged
- AgentHealthChanged
- AgentSessionStarted
- AgentSessionPaused
- AgentSessionResumed
- AgentSessionLost
- AgentSessionRecovered
- AgentSessionStopped
- AgentProcessExited
- AgentTaskAssigned
- AgentTaskAccepted
- AgentTaskReleased
- AgentHandoffRequested
- AgentDisabled
- AgentReenabled

Events are persisted and correlated with MCF-v2 events where applicable.

## 30. Security rules

An agent process is never trusted merely because it is a supported executable.

Every material action is checked against:

project + session + workspace + capability + policy + lease + context + epoch.

Cross-project use is rejected.

Agents cannot escape the project scope without explicit policy authorization.

## 31. Testing contract

Every real adapter must pass the common adapter conformance suite.

Tests cover:

- discovery
- version
- handshake
- capability detection
- launch
- send
- receive
- streaming where supported
- interrupt
- stop
- process verification
- workspace binding
- lease binding
- crash recovery
- duplicate messages
- stale context
- capability loss
- evidence collection

A simulated adapter must implement the same behavioral contract for orchestration tests.

## 32. Source-of-truth rules

Agent-specific implementation belongs in:

crates/agents

MCF-v2 semantics belong in:

crates/protocol and schemas/mcf-v2

Task ownership belongs in:

crates/tasks

Project lifecycle belongs in:

crates/core

Agent adapters may translate semantics, but may not redefine them.

## 33. Non-goals

The Agent subsystem is not:

- a model router that changes providers inside an agent
- a fifth AI
- a direct agent-to-agent chat network
- a cloud executor
- a replacement for the CLIs
- a certification authority

## 34. Implementation rule

Before an adapter is implemented, its actual installed CLI interface must be probed and recorded.

Documentation assumptions must never silently become runtime behavior.

If a native CLI changes, update only the adapter unless the canonical MCF-v2 contract itself must change.