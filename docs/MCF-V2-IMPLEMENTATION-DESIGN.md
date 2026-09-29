# MCF-v2 Implementation Design

## Authority
MCF-v2 semantics remain defined by `MCF-V2-PROTOCOL.md` and its machine-readable contract. This document defines implementation mechanics only.

## Runtime pipeline
```
encode
→ schema validate
→ persist inbox/outbox record
→ route
→ dispatch
→ receive
→ ACK/NACK
→ process
→ persist result
→ emit event
```

## Durable outbox
A domain transaction writes:
1. authoritative state mutation
2. immutable event
3. outbound message record

All three commit atomically.

A dispatcher may send only persisted outbox records.

After send, delivery state is updated separately; a crash may cause duplicate delivery and must be safe.

## Durable inbox
Receiver processing:
```
receive
→ validate envelope
→ validate project/session
→ persist message identity
→ if duplicate: return prior outcome
→ validate epoch/context/lease
→ execute side effect
→ persist outcome
→ ACK
```
Persistence before side effects prevents duplicate side effects after acknowledgement races.

## Idempotency
Idempotency keys are scoped to project + operation. Receivers retain terminal outcome references for replayable operations.

## Ordering
Ordering is enforced per session/channel sequence. A gap does not permit silent reordering. The receiver requests/reconciles the missing range according to sync policy.

## Routing
Routing checks project identity, recipient identity, channel, message type, capability and policy before dispatch.

No cross-project route is legal.

## Retry
Retry state contains attempt number, next-attempt time, reason and terminal disposition. Backoff is bounded and deterministic from stored retry policy/attempt state.

## Dead letters
Dead-letter records retain original message identity, final error, attempts and relevant causal references. Replay creates a new delivery attempt without rewriting history.

## ACK semantics
ACK confirms receipt/persistence, not processing success.
NACK identifies rejection and retryability.

## Context gate
Before material processing:
project epoch, context snapshot, state digest and relevant contract hashes are compared with current authoritative state. Stale input produces STALE_CONTEXT/EPOCH_CHANGED and no material write.

## Control priority
Emergency control and recovery traffic must not be blocked by bulk model output.

## Replay
Replay reconstructs state/events and can re-drive eligible messages through the same guards. Replay never bypasses authorization or validation.

## Observability
Every delivery attempt records message ID, sequence, timestamps, sender/recipient, outcome, latency, retry count and causal correlation.

## Transport boundary
The initial implementation is local-only. The transport abstraction must not imply a remote/cloud executor.


## Schema distribution and runtime validation

The canonical schema package lives at repository root under `schemas/mcf-v2/`. The repository copy is the only source of truth.

For the Windows MSI build:

1. CI validates the complete schema package before compilation.
2. The Rust `crates/protocol` build embeds the required schema files with Rust compile-time inclusion (for example `include_str!`) so runtime validation does not depend on the user's filesystem.
3. The same schema package may also be included as a read-only Tauri resource for diagnostics, but that resource is not authoritative.
4. Runtime validators report protocol/schema version and registry version in errors and evidence.
5. The MSI installer must not fetch schemas from the network.

The required package is therefore available in source, embedded in the protocol crate, and optionally exposed as an immutable diagnostic resource after packaging.

## Registry/code alignment gate

Before the bus is considered buildable:

- every message identifier is present in `schemas/mcf-v2/message-types.schema.json`;
- every event identifier is present in `schemas/mcf-v2/event-types.schema.json`;
- every priority lane is present in `schemas/mcf-v2/enums.schema.json`;
- every legal transition is represented by `transition-types.json`;
- the protocol crate, bus and adapters consume the same generated/validated identifiers;
- fixtures prove valid, invalid, duplicate, stale-context and replay cases.
