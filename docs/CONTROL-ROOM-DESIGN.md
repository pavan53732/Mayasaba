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

## Visual language

Base: minimal/functional. A neutral palette, generous whitespace, typographic hierarchy and a system font stack carry the interface; decorative surfaces are not used.

Layout: the persistent shell (header, navigation, center, right rail) uses a bento-grid composition.

Frosted-glass/blur accents are permitted only on the header bar and status chips. They must never fall below WCAG 2.1 AA contrast or obscure state.

Prohibited as design languages: spatial/3D, claymorphism, neomorphism, skeuomorphism, glassmorphism and maximalism.

Visual language is presentation only. It never encodes authority, state or permission; authoritative truth remains in Rust and the event stream. See DEC-032.

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
Council view shows round state, participation, proposals, critiques, disagreements, questions, decisions, locks and syntheses without private chain-of-thought. It shows the round's termination outcome and the reason the round ended.

## User question and answer flow

When CouncilService requires user input, show a consolidated question queue rather than interrupting for every agent message. Each question displays its impact/blocking status, concise reason it remains unresolved, affected agents and links to the source positions/evidence where available. The project clearly shows WAITING_FOR_USER while a blocking question is open; silence is never rendered as agreement.

When a question carries an `escalation_ref`, render the escalation packet's conflict matrix: the requirement IDs actually in dispute and each side's stance, alongside the competing positions' strongest arguments. The packet's recommendation is displayed as a labelled non-authoritative default, never as a decision. A question whose packet times out remains open; the UI must not present a timeout as an answer or as assent.

Submitting an answer calls the existing `answer_user_question` command. Show the answer's persisted status, its redistribution scope, and the resulting context snapshot/epoch. Track delivery/synchronization for affected agents separately from transport ACK; do not report the answer as applied until the agents resume from current context. A council answer is not a blanket permission or action approval—PolicyService approval remains a separate control.

## Intake and free-text contribution

The intake surface is the Control Room's Initial Intake Composer, shown when creating a new project. The user states the project idea and selects the local workspace. The stated intent is persisted as the first `ProjectBrief` version and displayed back as project truth, not as a chat message. Show the brief version and mark the version that became the analysis anchor when DISCOVERY closes.

After creation, free-text input is recorded as a `UserContribution` and displayed with its advisory classification and its outcome. Display the outcome the owning service actually produced, not the advisory label: a contribution the service found non-material must not appear to have changed project truth. When a contribution does change truth, show the resulting epoch, the affected scope and the context regeneration; when it does not, show it as timeline/commentary.

The chat surface is the user's interface. Typed state transitions determine what becomes project truth. UI enablement remains advisory; the owning service is authoritative.

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
