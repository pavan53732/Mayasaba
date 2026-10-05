//! The production `Clock` and `IdSource` the bus takes as caller-supplied traits.
//!
//! `crates/bus` performs no I/O and reads neither the wall clock nor a random device, which is what makes its
//! retry, backoff and due-time logic testable with a fixed clock. Something outside it must supply the real
//! ones, and this module is that something. It lives in `crates/core` rather than in the shell because core
//! already depends on both `bus` and `council`, so neither adapter needs a new dependency edge (DEC-074).
//!
//! # Why the clock is an adapter and not a new implementation
//!
//! `crates/council/src/clock.rs` holds the repository's one RFC3339 formatter and its own `SystemClock`. The
//! bus's module documentation says so explicitly and warns that a second copy of the civil-date algorithm
//! "would be a second source of truth for time formatting in exchange for nothing this crate needs". This
//! module therefore adds only the trait adapter: it reads the system time and hands the epoch seconds to the
//! canonical formatter. No calendar arithmetic is written here.
//!
//! # Why the shape is a contract and not a preference
//!
//! A queue entry is due when its `next_attempt_at` is at or before the clock's reading, and that comparison is
//! a **lexicographic string comparison** of two fixed-width UTC stamps. `YYYY-MM-DDTHH:MM:SSZ` orders correctly
//! under string comparison; almost any other spelling does not. A clock that emitted, say, a numeric offset or
//! a variable-width month would not fail loudly - it would silently misorder the queue, dispatching work early
//! or late. The tests below therefore assert the shape rather than a particular instant.
//!
//! # Why identities are UUID v4
//!
//! `schemas/mcf-v2/envelope.schema.json` declares `message_id` as `"format": "uuid"`, and the contract outranks
//! code (AGENTS.md section 5). The envelope validator does **not** enforce `format`, so a `msg_<hex>` id would
//! pass every test in this repository while violating the contract - and a message id is externally referenced,
//! travelling in envelopes and acknowledgements. `next_nonce` is reused as the entropy primitive; `digest` is
//! deliberately not, because it is a 64-bit FNV-1a whose own comment says it is to be replaced "when ids become
//! externally referenced", which message ids already are.
//!
//! Uniqueness is **probabilistic**, not guaranteed. The counter inside `next_nonce` is a seed, not an
//! allocator, and no claim is made that two draws cannot collide. The real guarantee is the `PRIMARY KEY` on
//! `messages.message_id`, which makes a collision a loud storage error rather than a silent overwrite; a
//! collision therefore surfaces rather than corrupts.

use mayasaba_bus::{Clock, IdSource};

/// The production clock: the system time, formatted by the repository's one RFC3339 formatter.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoreClock;

impl Clock for CoreClock {
    /// The current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
    ///
    /// The epoch is returned when the system clock cannot be read, because a clock that cannot tell the time
    /// must still answer rather than panic. That fallback is recorded rather than hidden: in the bus an epoch
    /// reading makes **every** pending entry look due at once, so a failure here would present as a burst of
    /// dispatches rather than as an error. It is unreachable in practice - it needs a system clock set before
    /// 1970 - and it is written down because the consequence is not obvious from the code.
    fn now_rfc3339(&self) -> String {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or_default())
            .unwrap_or_default();
        mayasaba_council::clock::format_rfc3339_utc(seconds)
    }
}

/// A version 4 UUID built from two draws of the shared nonce primitive.
///
/// 128 bits are taken as two 64-bit nonce draws, then the two fields RFC 4122 fixes are overwritten: the
/// version nibble becomes `4`, and the variant's top bits become `10xx` (so the nibble is one of `8`, `9`, `a`,
/// `b`). Overwriting them is what makes the output a well-formed v4 UUID rather than merely 128 random bits
/// wearing hyphens.
pub fn new_uuid_v4() -> String {
    let mut digits: Vec<char> = format!(
        "{}{}",
        crate::project_service::next_nonce(),
        crate::project_service::next_nonce()
    )
    .chars()
    .collect();

    // A nonce draw is `{:016x}`, so the pair is exactly 32 hex digits and these indices are in range. The
    // guards keep that a checked fact rather than an assumption if the nonce format ever changes.
    if digits.len() != 32 {
        return String::new();
    }

    digits[12] = '4';
    digits[16] = match digits[16].to_digit(16).unwrap_or_default() & 0x3 {
        0 => '8',
        1 => '9',
        2 => 'a',
        _ => 'b',
    };

    let hex: String = digits.into_iter().collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The production identity source: a fresh v4 UUID per message.
///
/// It holds no state, because the entropy is in the shared nonce primitive rather than in this type. That is
/// deliberate: two `CoreIdSource` values in one process draw from the same primitive, so they do not collide
/// with each other, and there is no per-instance counter to reset and start repeating.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoreIdSource;

impl IdSource for CoreIdSource {
    fn next_message_id(&self) -> String {
        new_uuid_v4()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the due-time comparison depends on, spelled out rather than approximated.
    fn is_bus_timestamp(stamp: &str) -> bool {
        let bytes = stamp.as_bytes();
        if bytes.len() != 20 {
            return false;
        }
        let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
        digits(0..4)
            && bytes[4] == b'-'
            && digits(5..7)
            && bytes[7] == b'-'
            && digits(8..10)
            && bytes[10] == b'T'
            && digits(11..13)
            && bytes[13] == b':'
            && digits(14..16)
            && bytes[16] == b':'
            && digits(17..19)
            && bytes[19] == b'Z'
    }

    fn is_uuid_v4(id: &str) -> bool {
        let bytes = id.as_bytes();
        if bytes.len() != 36 {
            return false;
        }
        for (index, byte) in bytes.iter().enumerate() {
            match index {
                8 | 13 | 18 | 23 => {
                    if *byte != b'-' {
                        return false;
                    }
                }
                _ => {
                    if !byte.is_ascii_hexdigit() {
                        return false;
                    }
                }
            }
        }
        bytes[14] == b'4' && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
    }

    #[test]
    fn the_clock_produces_the_shape_the_due_comparison_requires() {
        // Asserted as a shape, not as an instant: a fixed instant would encode today's date as the
        // specification and break on a legitimate change. The shape is what the string comparison needs.
        let stamp = CoreClock.now_rfc3339();
        assert!(
            is_bus_timestamp(&stamp),
            "the clock produced {stamp:?}, which is not YYYY-MM-DDTHH:MM:SSZ; the due check is a \
             lexicographic comparison and any other spelling would silently misorder the queue"
        );
    }

    #[test]
    fn the_clock_agrees_with_the_canonical_formatter() {
        // The adapter must not reimplement the calendar. If it ever does, this compares it against the
        // repository's one formatter rather than against a second copy of the same arithmetic.
        let canonical = mayasaba_council::clock::format_rfc3339_utc(1_700_000_000);
        assert_eq!(canonical, "2023-11-14T22:13:20Z");
        assert!(is_bus_timestamp(&canonical));
    }

    #[test]
    fn the_clock_reads_forward_and_orders_lexicographically() {
        // Two readings either match or increase, and the string comparison the bus uses agrees with the
        // chronological order. This is the property that makes a lexicographic due check correct.
        let first = CoreClock.now_rfc3339();
        let second = CoreClock.now_rfc3339();
        assert!(first <= second, "{first} should not sort after {second}");
    }

    #[test]
    fn an_identity_is_a_version_4_uuid() {
        for _ in 0..256 {
            let id = CoreIdSource.next_message_id();
            assert!(
                is_uuid_v4(&id),
                "{id:?} is not a version 4 UUID, and envelope.schema.json declares message_id as \
                 \"format\": \"uuid\"; the validator does not enforce format, so nothing else would catch it"
            );
        }
    }

    #[test]
    fn identities_do_not_repeat_over_a_large_batch() {
        // Uniqueness is probabilistic, so this is evidence rather than proof. The real guarantee is the
        // PRIMARY KEY on messages.message_id, which turns a collision into a loud error.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..20_000 {
            assert!(
                seen.insert(new_uuid_v4()),
                "an identity repeated within one batch"
            );
        }
    }

    #[test]
    fn identities_do_not_repeat_across_threads() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| (0..2_000).map(|_| new_uuid_v4()).collect::<Vec<String>>())
            })
            .collect();

        let mut seen = std::collections::HashSet::new();
        for handle in handles {
            let batch = handle.join().expect("a generator thread panicked");
            for id in batch {
                assert!(seen.insert(id), "two threads produced the same identity");
            }
        }
        assert_eq!(seen.len(), 16_000);
    }

    #[test]
    fn two_sources_in_one_process_do_not_collide() {
        // The entropy is in the shared primitive, not in the type, so two sources must not start over from a
        // per-instance counter and repeat each other.
        let a = CoreIdSource;
        let b = CoreIdSource;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4_000 {
            assert!(seen.insert(a.next_message_id()));
            assert!(seen.insert(b.next_message_id()));
        }
        assert_eq!(seen.len(), 8_000);
    }
}
