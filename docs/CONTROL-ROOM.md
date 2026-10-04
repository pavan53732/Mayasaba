# Mayasaba Control Room

## Purpose

The Control Room is the primary Windows desktop interface for observing and controlling user-authorized work on local files. It presents the full software-engineering lifecycle where relevant and task-appropriate workflows for documents, research reports and data cleanup.

## Primary navigation

Projects

Current project sections:

- Chat
- Council
- Requirements
- Architecture
- Decisions
- Tasks
- Agents
- Files
- Build
- Run / Preview
- Tests
- Repairs
- Logs
- Evidence

The same workspace, task, evidence and review surfaces cover non-software artifacts; build/run/test sections are shown or gated as applicable to the task.

## Workspace and task scope

Show the user-selected local workspace and task-scoped allowed paths. Do not treat the whole PC as a default scan scope. For user-requested research, display public-source retrieval as read-only and present locally saved citations/report artifacts. External side effects and general control of unrelated applications are not available actions.

## Project creation and intake

Project creation begins in the Initial Intake Composer. The user selects a Windows-local workspace folder and describes the project intent. The workspace leaf folder name initializes the project’s display name; the project receives an independently generated opaque \project_id\ that is never derived from a path (DEC-050). They may additionally attach local files or folders as supporting context.

The selected workspace is the project's initial filesystem boundary. A typed path is not treated as authorized merely because it is syntactically valid: path existence, locality and policy authorization are separate checks, and the native folder-selection flow is the primary interaction rather than manual entry.

The project intent is persisted as `ProjectBrief` version 1 and displayed as project truth after successful creation. Before that commit it is local draft state only.

Attached files and folders are supporting context and evidence with source provenance. They do not replace the `ProjectBrief` and do not become project truth solely by being attached.

The Control Room distinguishes draft, submitting, created and rejected states, and never invents or infers project state while creation is pending. Mark the brief version that became the immutable analysis anchor when DISCOVERY closes.

Free-text input after creation is recorded as a `UserContribution`. Show its advisory classification alongside the outcome the owning service actually produced, so a non-material contribution is never displayed as having changed project truth. When a contribution changes truth, show the resulting epoch and affected scope.

The chat surface is the user's interface; typed state transitions determine what becomes project truth.

## Visual language

The Control Room uses the locked minimal/functional base with a bento-grid layout (DEC-032). Composition details are owned by `CONTROL-ROOM-DESIGN.md`.

## Header

Persistent project header shows:

- project name
- local path
- lifecycle phase
- agent health
- execution state
- pause/stop controls

## Main center

The center view is the work timeline/chat:

- user messages
- agent messages
- council rounds
- phase markers
- decisions
- task events
- build/test events when relevant
- document/report/data artifact changes
- research source/citation records
- failures
- repairs
- evidence

Agent identity and message state are visible.

## Right-side status

Show:

- current phase
- active agents
- task owners
- task status
- applicable build/test or artifact-validation health
- blockers
- communication lag
- message ACK/processing state
- project epoch
- active context snapshot
- lease expiry
- barriers
- failures/retries/dead letters

## Council view

Round-by-round view of:

- independent proposals
- critiques
- rebuttals
- revisions
- disagreements
- questions
- decisions
- locks
- syntheses
- participation status
- round termination outcome and reason

## Evidence view

Every important completion claim links to observable evidence:

- source diff
- command
- stdout/stderr
- test result
- build result
- artifact
- source citations/metadata for research reports
- format/schema checks, record counts, invariants or recoverability evidence for document/data tasks where applicable
- screenshot/UI evidence where applicable
- validation result

## User controls

User may:

- answer a consolidated open Council question and see which agents are affected
- pause
- resume
- retry
- reassign
- reopen an explicit decision
- approve a blocked operation only when the requested scope/action is within product policy
- stop

Controls must map to actual controller state transitions, not merely UI state.

## Visibility rule

The Control Room does not expose private model chain-of-thought. It exposes model-provided reasoning/rationales, messages, conclusions, tool actions and evidence.

## Preview

Where the target project has a runnable UI/application, the local running target is a first-class preview surface. Preview status is evidence-backed rather than inferred from a successful start command alone.

The implementation-level UI composition and event-cursor contract is defined in `CONTROL-ROOM-DESIGN.md`.
