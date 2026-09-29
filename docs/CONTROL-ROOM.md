# Mayasaba Control Room

## Purpose

The Control Room is the primary Windows desktop interface for observing and controlling the autonomous engineering lifecycle.

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

## Header

Persistent project header shows:

- project name
- local path
- lifecycle phase
- agent health
- execution state
- pause/stop controls

## Main center

The center view is the engineering timeline/chat:

- user messages
- agent messages
- council rounds
- phase markers
- decisions
- task events
- build/test events
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
- build/test health
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
- participation status

## Evidence view

Every important completion claim links to observable evidence:

- source diff
- command
- stdout/stderr
- test result
- build result
- artifact
- screenshot/UI evidence where applicable
- validation result

## User controls

User may:

- pause
- resume
- retry
- reassign
- reopen an explicit decision
- approve a blocked operation
- stop

Controls must map to actual controller state transitions, not merely UI state.

## Visibility rule

The Control Room does not expose private model chain-of-thought. It exposes model-provided reasoning/rationales, messages, conclusions, tool actions and evidence.

## Preview

Where the target project has a runnable UI/application, the local running target is a first-class preview surface. Preview status is evidence-backed rather than inferred from a successful start command alone.


The implementation-level UI composition and event-cursor contract is defined in `CONTROL-ROOM-DESIGN.md`.