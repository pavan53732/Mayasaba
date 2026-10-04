//! Behavioural tests for the generated protocol surface.
//!
//! `tools/contracts/verify.mjs` proves the generated file is byte-identical to what the contract implies.
//! That is a different property from the generated values being usable, and the difference is not
//! theoretical: the first version of this generated code passed the gate while containing an enum with no
//! variants, a 121-error naming mismatch, and an `Option<&str>` returned as `&str`. Only rustc found those.
//!
//! These tests assert the generated surface against the contract's own values, so a generator regression is
//! caught as a failing assertion rather than as a compile error or, worse, as a silently wrong constant.

use mayasaba_protocol::generated::machines::{
    branches, owner_crate, spine, state_count, states, unreviewed_branches, Machine, AGENT_TYPES,
    EVENT_TYPES, SERVICE_EMITTED_EVENTS, TRANSITION_COMMANDS, TRANSITION_EMITTED_EVENTS,
};

#[test]
fn all_lists_every_machine_in_the_contract() {
    // The contract declares twelve machines. If a machine is added to the contract and the crate is not
    // regenerated, ALL and this count both change together only if the test data changes too, which is why
    // the count is asserted independently below.
    assert_eq!(
        Machine::ALL.len(),
        12,
        "expected one variant per declared machine"
    );
    let mut ids: Vec<&str> = Machine::ALL.iter().map(|m| m.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), Machine::ALL.len(), "machine ids must be unique");
}

#[test]
fn from_id_round_trips_for_every_machine() {
    for machine in Machine::ALL {
        let id = machine.id();
        let back = Machine::from_id(id)
            .unwrap_or_else(|| panic!("{id} did not resolve back to a Machine"));
        assert_eq!(back, *machine, "from_id(id(m)) must equal m");
    }
}

#[test]
fn display_matches_id() {
    for machine in Machine::ALL {
        assert_eq!(
            machine.to_string(),
            machine.id(),
            "Display must render the contract id"
        );
    }
}

#[test]
fn from_id_rejects_unknown_identifiers() {
    assert!(Machine::from_id("not_a_machine").is_none());
    assert!(Machine::from_id("").is_none());
}

#[test]
fn owner_crate_is_declared_for_every_machine() {
    for machine in Machine::ALL {
        let owner = owner_crate(*machine)
            .unwrap_or_else(|| panic!("{} has no declared owner", machine.id()));
        assert!(
            owner.starts_with("crates/"),
            "{machine} owner {owner} is not a crate path"
        );
    }
}

#[test]
fn spine_states_and_branches_are_internally_consistent() {
    for machine in Machine::ALL {
        let declared = states(*machine);
        let ordered = spine(*machine);

        // Every spine entry must be a declared state: the spine is an ordered path *through* the state set,
        // not an independent list. This is the invariant DEC-042 introduced when spine became mandatory.
        for s in ordered {
            assert!(
                declared.contains(s),
                "{} spine contains {s}, which is not in its declared states",
                machine.id()
            );
        }

        // Every branch endpoint must be a declared state, or the machine permits an edge into nowhere.
        for b in branches(*machine) {
            if let Some((from, to)) = b.split_once("->") {
                assert!(
                    declared.contains(&from),
                    "{} branch {b} leaves undeclared state {from}",
                    machine.id()
                );
                assert!(
                    declared.contains(&to),
                    "{} branch {b} enters undeclared state {to}",
                    machine.id()
                );
            } else {
                panic!("{} branch {b} is not in SOURCE->TARGET form", machine.id());
            }
        }

        // A state must not be reviewed and blessed at once: the two lists are meant to be disjoint.
        for u in unreviewed_branches(*machine) {
            assert!(
                !branches(*machine).contains(u),
                "{} lists {u} as both blessed and unreviewed",
                machine.id()
            );
        }

        assert!(state_count(*machine) == declared.len());
    }
}

#[test]
fn spine_adjacency_is_declared_as_a_path() {
    for machine in Machine::ALL {
        let ordered = spine(*machine);
        if ordered.len() < 2 {
            // A one-state spine is the explicit declaration of "no spine edges" (DEC-042). context uses it.
            continue;
        }
        // Consecutive spine entries must differ; a repeated state means the ordered path is malformed.
        for pair in ordered.windows(2) {
            assert_ne!(pair[0], pair[1], "{} spine repeats {pair:?}", machine.id());
        }
    }
}

#[test]
fn emitter_sets_partition_the_canonical_events() {
    let total = TRANSITION_EMITTED_EVENTS.len() + SERVICE_EMITTED_EVENTS.len();
    assert_eq!(
        total,
        EVENT_TYPES.len(),
        "every canonical event must be emitted by a transition or a service, and the two sets must partition them"
    );

    for e in TRANSITION_EMITTED_EVENTS {
        assert!(
            !SERVICE_EMITTED_EVENTS.contains(e),
            "{e} is declared as both transition-emitted and service-emitted"
        );
        assert!(EVENT_TYPES.contains(e), "{e} is not a canonical event");
    }
    for e in SERVICE_EMITTED_EVENTS {
        assert!(EVENT_TYPES.contains(e), "{e} is not a canonical event");
    }
}

#[test]
fn agent_types_are_exactly_the_three_supported_adapters() {
    // DEC-029 reduced the agent set to three. If the contract changes, this fails and asks the question
    // again rather than letting a fourth adapter appear because a schema edit allowed it.
    assert_eq!(
        AGENT_TYPES.len(),
        3,
        "DEC-029 fixes the agent set at three: {AGENT_TYPES:?}"
    );
    for expected in ["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"] {
        assert!(
            AGENT_TYPES.contains(&expected),
            "missing agent type {expected}"
        );
    }
    for forbidden in ["CLAUDE_CODE", "CLINE"] {
        assert!(
            !AGENT_TYPES.contains(&forbidden),
            "{forbidden} was withdrawn by DEC-029"
        );
    }
}

#[test]
fn transition_commands_are_unique_and_non_empty() {
    assert!(
        !TRANSITION_COMMANDS.is_empty(),
        "the contract registers transition commands"
    );
    let mut sorted = TRANSITION_COMMANDS.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        count,
        "transition command names must be unique"
    );
}

#[test]
fn every_machine_has_a_usable_ordered_path() {
    // Not a hard guarantee, but a machine whose spine is empty while it has states is almost always a
    // declaration mistake, so it is worth surfacing rather than passing silently.
    for machine in Machine::ALL {
        assert!(
            !spine(*machine).is_empty(),
            "{} declares states but no spine at all; declare an explicit one-state spine if it has no edges",
            machine.id()
        );
    }
}
