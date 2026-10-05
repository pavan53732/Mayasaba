//! Tests for the capacity bound on the outbound queue.
//!
//! The bound exists so that a producer which outruns the dispatcher is told to wait rather than growing the
//! queue without limit. Three properties are load-bearing and each is checked here: a refusal is reported as
//! transient rather than as a denial, an enqueue that adds no work is never refused, and the bound is released
//! once the queue drains.

mod common;

use common::{envelope, storage_with_project, ScriptedTransport};
use mayasaba_bus::{BackoffPolicy, Bus, BusError, DispatchPolicy, FixedClock};

fn at(stamp: &str) -> FixedClock {
    FixedClock::new(stamp)
}

fn bus_with_bound(tag: &str, max_pending: i64) -> (Bus, std::path::PathBuf) {
    let (storage, dir) = storage_with_project(tag);
    let bus = Bus::with_policy(
        storage,
        DispatchPolicy {
            max_pending,
            ..DispatchPolicy::default()
        },
        BackoffPolicy::default(),
    );
    (bus, dir)
}

/// A refusal at the bound is `CAPACITY_EXCEEDED`, not `POLICY_DENIED`, and the same request succeeds once the
/// queue drains. Both halves matter: a caller that cannot tell "not yet" from "never" cannot act correctly.
#[test]
fn an_enqueue_beyond_the_bound_is_refused_transiently() {
    let (mut bus, _dir) = bus_with_bound("cap_refuse", 2);
    let clock = at("2026-11-01T00:00:10Z");

    for n in 1..=2 {
        let e = envelope(
            &format!("msg_cap_{n}"),
            "prj_cap_refuse",
            n,
            Some(&format!("op_cap_{n}")),
            r#"{"task":"a"}"#,
        );
        bus.enqueue(&e).expect("within the bound");
    }

    let e = envelope(
        "msg_cap_3",
        "prj_cap_refuse",
        3,
        Some("op_cap_3"),
        r#"{"task":"a"}"#,
    );
    match bus.enqueue(&e) {
        Err(BusError::CapacityExceeded { pending, limit }) => {
            assert_eq!(pending, 2, "both queued entries were waiting");
            assert_eq!(limit, 2, "the configured bound is reported back");
        }
        other => panic!("expected a capacity refusal, got {other:?}"),
    }

    // The refusal was transient: draining the queue makes the identical request acceptable.
    let mut transport = ScriptedTransport::always_ok();
    bus.dispatch_due(&clock, &mut transport).expect("pass");
    bus.enqueue(&e).expect("accepted once the queue drained");
}

/// An enqueue that adds no work is never refused, even at the bound.
///
/// This is the reason the check is skipped for a `message_id` the bus already knows: a retry of an existing
/// message is answered with the original, so refusing it for capacity would refuse something that consumes no
/// capacity, and would tell a caller to wait for a queue its own request is not growing.
#[test]
fn an_enqueue_that_adds_no_work_is_never_refused() {
    let (mut bus, _dir) = bus_with_bound("cap_retry", 1);
    let e = envelope(
        "msg_retry_1",
        "prj_cap_retry",
        1,
        Some("op_retry_1"),
        r#"{"task":"a"}"#,
    );

    bus.enqueue(&e).expect("first");
    // The queue is now at its bound of one. This request is a retry of the message already in it.
    bus.enqueue(&e)
        .expect("a retry adds no work and must not be refused for capacity");
}
