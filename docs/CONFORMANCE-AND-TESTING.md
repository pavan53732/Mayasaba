# Mayasaba Conformance and Testing Strategy

## Purpose

Mayasaba's autonomy is accepted only when its coordination and engineering behavior are testable.

## Protocol conformance

Every MCF-v2 implementation must test:

- schema validity
- message-to-payload mapping
- enum correctness
- channel compatibility
- transition legality
- required guards
- ACK/NACK
- retry
- idempotency
- ordering
- sequence gaps
- correlation/causation
- dead letters
- replay

## Adapter conformance

Each of the four initial adapters must pass the same behavioral contract:

- discovery
- version
- handshake
- capability negotiation
- workspace validation
- ready state
- send/receive
- streaming
- interrupt
- resume where supported
- stop
- process-state verification
- change/evidence collection

Native capabilities may differ, but unsupported features must be reported explicitly.

## Recovery tests

Must simulate:

- CLI crash
- adapter crash
- Mayasaba restart
- UI restart
- duplicate delivery
- delayed delivery
- out-of-order delivery
- crash after inbox persistence
- crash after side effect before acknowledgement
- outbox not dispatched
- lease expiry
- stale context
- epoch change
- partial council participation

## Workspace tests

- concurrent Git worktree isolation
- integration conflict handling
- non-Git checkpointing
- path-scope enforcement
- outside-root escalation
- process cleanup

## Engineering loop tests

At minimum, exercise:

idea → requirements → architecture → task → implementation → integration → build → test → E2E → review → failure → diagnosis → repair → regression → certification.

## Anti-hallucination tests

The system must prove that:

- agent completion text does not certify a task
- nonexistent commands are not invented when repository facts disagree
- tests cannot be deleted/disabled to force a pass without an explicit reviewed decision
- HARD_LOCKs cannot be silently bypassed
- stale context cannot authorize writes
- expired leases cannot authorize writes
- conflicting agent claims trigger resolution rather than silent selection

## Security tests

- cross-project message rejection
- secret leakage detection
- policy denial
- unauthorized tool invocation
- destructive/admin escalation
- hidden command detection

## Release gate

A Mayasaba release must have passing conformance results for protocol, adapters, storage, recovery, workspace, execution, validation and certification. Failed mandatory suites block release.
