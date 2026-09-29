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

Right context rail shows active agents, tasks, blockers, build/test health, communication health, epoch, context snapshot, lease expiry, barriers, retries and dead letters.

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
