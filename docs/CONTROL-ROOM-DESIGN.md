# Mayasaba Control Room UX Architecture

## Authority
This document defines UI composition and state presentation. Authoritative lifecycle/state remains in Rust/domain services.

## Layout
Persistent shell:
- project/path
- phase/status
- agent health
- execution status
- pause/resume/stop

Primary navigation:
Chat, Council, Requirements, Architecture, Decisions, Tasks, Agents, Files, Build, Run/Preview, Tests, Repairs, Logs, Evidence, Settings.

Center content is feature-specific. A project timeline can aggregate authoritative events without becoming a second state machine.

Right context rail shows the user-selected workspace/task scope, active agents, tasks, blockers, task-applicable build/test or artifact-validation health, communication health, epoch, context snapshot, lease expiry, barriers, retries and dead letters.

## State presentation
UI distinguishes:
- authoritative server state
- event timeline
- local view state
- unsaved drafts

The UI never invents lifecycle truth.

## Action semantics
Every user action maps to a typed Tauri command and displays:
request → pending → accepted/rejected → resulting event/state.

Buttons are disabled when the controller reports the action is not admissible; UI disablement is not itself an authorization mechanism.

## Workspace and action boundary

Present task-scoped allowed paths within the user-selected local workspace; the whole PC is not an implicit scan scope. User-requested public research may retrieve publicly accessible sources read-only and save citations/results locally. The UI must not expose email, messaging, external posting/form submission, purchases, account changes, or general controls for unrelated applications.

Build, runtime and E2E surfaces remain first-class for software tasks. Other local artifacts use applicable review, validation and evidence surfaces without irrelevant software-only gates.

## Agent/council views
Agent view shows runtime identity, session, capabilities, health, active task, lease and recent evidence.
Council view shows round state, participation, proposals, critiques, disagreements, questions, decisions and locks without private chain-of-thought.

## Evidence UX
Claims link to evidence. Failed validations expose failure packet, affected scope and repair state.

## Recovery UX
Show recovering state, affected scope, current recovery step and next controller action. Do not fabricate progress.

## Preview
Preview is a first-class local runtime surface. Display process status and runtime verification separately from launch status.

## Accessibility and safety
Keyboard navigation, visible focus, confirmation for destructive actions, clear blocked reasons, and no hidden privileged actions.

## Event cursor
UI subscriptions use event cursors. Sequence gaps trigger recovery/resync rather than silent local reconstruction.

## Machine-readable UI wiring

The canonical Tauri identifier registry is `schemas/tauri-bridge-v1/bridge.schema.json`; operation metadata is `schemas/tauri-bridge-v1/payloads.json`; ownership is `workspace.manifest.json`.

The Control Room uses `get_event_cursor` to load the durable cursor, `request_event_resync` on sequence gaps, `get_action_admissibility` before enabling a mutating action, `get_doctor_report` for preflight, and `get_recovery_status` for recovery progress.

UI enablement is advisory only; Rust authorization remains authoritative.
