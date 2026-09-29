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
