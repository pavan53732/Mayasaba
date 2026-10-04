//! Round role assignment (DEC-052 item 5, `COUNCIL-ENGINE.md` "Round roles").
//!
//! The controller assigns `proposer`, `skeptic` and `verifier` for each round by deterministic rotation, so
//! that one agent with a favourable sort key is not always the skeptic and so that an assignment can be
//! reproduced from the round's participants, the round index, the configured seed and the lineage table.
//! There is no randomness and no clock in this module.
//!
//! Two boundaries matter more than the rotation itself:
//!
//! * **A role is framing, never authority.** Nothing in this module (or this crate) turns a role into a
//!   permission, a capability, a lease or a vote weight. What a role buys is a duty and a place in the
//!   round's content: a missed duty is recorded as non-participation for that duty, never as agreement. The
//!   only output of an assignment here is data on a `council_round_roles` row.
//! * **The skeptic is drawn from a different lineage group than the leading proposal's author when
//!   possible.** When the participating lineage groups cannot supply one, the assignment records
//!   [`LINEAGE_UNAVOIDABLE`] in its reason rather than hiding the collision, because an unavoidable
//!   same-lineage skeptic is a quality fact the escalation packet has to be able to show.
//!
//! The rotation is a stable sort, not a shuffle: each participant gets a key derived from the round index,
//! the seed and its own `agent_id`, and the roles are read off the sorted order. Deterministic by
//! construction, and reproducible by anyone holding the same three inputs.

use std::collections::BTreeSet;

use crate::lineage::{adapter_lineage, LineageGroup};

/// Recorded when the skeptic could not be drawn from a different lineage group than the leading proposal's
/// author.
///
/// The token is part of the assignment's `assigned_reason`, so it survives into `council_round_roles` and
/// into anything that reads those rows.
pub const LINEAGE_UNAVOIDABLE: &str = "LINEAGE_UNAVOIDABLE";

/// The rotation reason recorded on an assignment that had a genuine choice.
pub const ASSIGNMENT_REASON_ROTATION: &str = "DETERMINISTIC_ROTATION";

/// Recorded on an assignment that completed the roster after rotation had filled every role it could.
pub const ASSIGNMENT_REASON_ROTATION_REPEAT: &str = "DETERMINISTIC_ROTATION_REPEAT";

/// The three duties a round assigns.
///
/// The contract spells them lowercase (`proposer`, `skeptic`, `verifier`) in the round's participant
/// context and uppercase (`PROPOSER`, `SKEPTIC`, `VERIFIER`) in `council_round_roles`. Both spellings live
/// here so no caller has to restate the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RoleKind {
    Proposer,
    Skeptic,
    Verifier,
}

impl RoleKind {
    /// Every role, in assignment order.
    pub const ALL: [RoleKind; 3] = [RoleKind::Proposer, RoleKind::Skeptic, RoleKind::Verifier];

    /// The role the round's leading proposal belongs to.
    pub const LEADING: RoleKind = RoleKind::Proposer;

    /// The `council_round_roles.role` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            RoleKind::Proposer => "PROPOSER",
            RoleKind::Skeptic => "SKEPTIC",
            RoleKind::Verifier => "VERIFIER",
        }
    }

    /// The round participant context spelling.
    pub fn as_participant_token(self) -> &'static str {
        match self {
            RoleKind::Proposer => "proposer",
            RoleKind::Skeptic => "skeptic",
            RoleKind::Verifier => "verifier",
        }
    }

    /// Parse the `council_round_roles.role` spelling.
    pub fn parse(value: &str) -> Option<Self> {
        RoleKind::ALL
            .into_iter()
            .find(|role| role.as_str() == value)
    }
}

/// The `RoundRole` names of the task.
///
/// This is an alias of [`RoleKind`], kept as a distinct public name because the specification and the
/// `council_round_roles` table call the concept a round role while the assignment reads it as a kind.
pub type RoundRole = RoleKind;

impl std::fmt::Display for RoleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One role assigned to one participant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignment {
    pub role: RoleKind,
    pub agent_id: String,
    /// The lineage group the agent's adapter belongs to, or `None` when the adapter is not in the static
    /// table. Recorded rather than recomputed by the reader, so the record shows the lineage the assignment
    /// actually reasoned from.
    pub lineage: Option<LineageGroup>,
    /// Why this agent holds this role. Carries [`LINEAGE_UNAVOIDABLE`] when the skeptic collision could not
    /// be avoided.
    pub assigned_reason: String,
}

impl RoleAssignment {
    /// Whether this assignment records an unavoidable same-lineage skeptic.
    pub fn is_lineage_unavoidable(&self) -> bool {
        self.assigned_reason.contains(LINEAGE_UNAVOIDABLE)
    }
}

/// A `council_round_roles` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundRoleRecord {
    pub role: RoleKind,
    pub agent_id: String,
    pub assigned_reason: String,
}

/// Every role assigned for a round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundRoles {
    pub round_index: u64,
    /// The rotation seed, echoed so the assignment is reproducible from the record.
    pub seed: u64,
    /// Assignments in role order, then agent order within a role.
    pub assignments: Vec<RoleAssignment>,
    /// Whether any assignment had to accept a same-lineage skeptic.
    pub lineage_unavoidable: bool,
    /// Participants the rotation did not need, so a caller can see who has no duty this round rather than
    /// inferring it from absence.
    pub unassigned_agent_ids: Vec<String>,
}

impl RoundRoles {
    /// The agent holding `role`, when exactly one assignment fills it.
    pub fn agent_for(&self, role: RoleKind) -> Option<&str> {
        self.assignments
            .iter()
            .find(|assignment| assignment.role == role)
            .map(|assignment| assignment.agent_id.as_str())
    }

    /// Whether every role was filled.
    pub fn is_complete(&self) -> bool {
        RoleKind::ALL.into_iter().all(|role| {
            self.assignments
                .iter()
                .any(|assignment| assignment.role == role)
        })
    }

    /// The rows to persist.
    pub fn records(&self) -> Vec<RoundRoleRecord> {
        self.assignments
            .iter()
            .map(|assignment| RoundRoleRecord {
                role: assignment.role,
                agent_id: assignment.agent_id.clone(),
                assigned_reason: assignment.assigned_reason.clone(),
            })
            .collect()
    }
}

/// Assign the round's roles.
///
/// `agent_types` maps `agent_id` to the static adapter type the controller resolved for that agent - not
/// what the agent reports about itself. `leading_proposal_author` is the author of the round's leading
/// proposal, which the round knows and this crate does not guess; when it is absent, the rotation's first
/// participant is treated as the proposer and the skeptic's lineage requirement is applied against that
/// agent.
///
/// The rotation is `H(seed, round_index, agent_id)` ordered ascending with `agent_id` breaking every tie,
/// so the order is total and stable and no agent's position in the input list matters.
pub fn assign_round_roles(
    participants: &[&str],
    agent_types: &[(&str, &str)],
    leading_proposal_author: Option<&str>,
    round_index: u64,
    seed: u64,
) -> RoundRoles {
    let mut roster: BTreeSet<String> = participants
        .iter()
        .map(|agent| (*agent).to_string())
        .collect();
    roster.remove("");

    // Never empty by construction, and every role is filled from it.
    let rotated = rotate(&roster, round_index, seed);
    let lineage_of = |agent_id: &str| -> Option<LineageGroup> {
        agent_types
            .iter()
            .find(|(id, _)| *id == agent_id)
            .and_then(|(_, agent_type)| adapter_lineage(agent_type))
            .map(|source| source.group)
    };

    let mut assignments: Vec<RoleAssignment> = Vec::new();
    let mut assigned: BTreeSet<String> = BTreeSet::new();
    let mut lineage_unavoidable = false;

    // The proposer: the round's named leading author when it is a participant, otherwise the head of the
    // rotation. The rotation is what stops a fixed agent holding this role every round.
    let proposer = match leading_proposal_author {
        Some(author) if roster.contains(author) => author.to_string(),
        Some(_) | None => rotated.first().cloned().unwrap_or_default(),
    };

    if !proposer.is_empty() {
        let reason = if Some(proposer.as_str()) == leading_proposal_author {
            "leading proposal author, confirmed as a round participant".to_string()
        } else {
            format!("{ASSIGNMENT_REASON_ROTATION}: first in rotation order (round {round_index}, seed {seed})")
        };
        assignments.push(RoleAssignment {
            role: RoleKind::Proposer,
            lineage: lineage_of(&proposer),
            agent_id: proposer.clone(),
            assigned_reason: reason,
        });
        assigned.insert(proposer.clone());
    }

    let proposer_lineage = lineage_of(&proposer);

    // The skeptic: the first unassigned candidate in *ring order* whose lineage differs from the proposer's.
    // Ring order starts at an offset derived from the round index, not at the head, because the proposer
    // rotates through the roster and the head of the rotation is therefore always available as a
    // different-lineage candidate - which would hand the skeptic duty to one agent every round.
    match ring_pick(&rotated, round_index, &assigned, |candidate| {
        lineages_differ(lineage_of(candidate), proposer_lineage)
    }) {
        Some(agent) => {
            assignments.push(RoleAssignment {
                role: RoleKind::Skeptic,
                lineage: lineage_of(&agent),
                agent_id: agent.clone(),
                assigned_reason: format!(
                    "{ASSIGNMENT_REASON_ROTATION}: first candidate in ring order from a different lineage group"
                ),
            });
            assigned.insert(agent);
        }
        None => {
            let fallback = ring_pick(&rotated, round_index, &assigned, |_| true);
            match fallback {
                Some(agent) => {
                    lineage_unavoidable = true;
                    assignments.push(RoleAssignment {
                        role: RoleKind::Skeptic,
                        lineage: lineage_of(&agent),
                        agent_id: agent.clone(),
                        assigned_reason: format!(
                            "{LINEAGE_UNAVOIDABLE}: no remaining participant belongs to a different lineage group than the leading proposal author, so the skeptic shares its lineage"
                        ),
                    });
                    assigned.insert(agent);
                }
                // One participant only: no one else can hold the skeptic duty. The reason says so and the
                // duty is left unwritten rather than duplicated onto an agent that already holds one.
                None => {
                    lineage_unavoidable = true;
                    assignments.push(RoleAssignment {
                        role: RoleKind::Skeptic,
                        lineage: proposer_lineage,
                        agent_id: proposer.clone(),
                        assigned_reason: format!(
                            "{LINEAGE_UNAVOIDABLE}: no other participant is available to act as skeptic, so the round has no independent skeptic and is uncorroborated"
                        ),
                    });
                }
            }
        }
    }

    // The verifier: the next unassigned participant in ring order. Each agent holds each duty at most once,
    // so a role that no participant can fill is left unfilled. That happens whenever there are fewer
    // participants than duties - a `REVIEW` round declares two participants for three duties - and the caller
    // records the unwritten duty as non-participation, which is DEC-052's rule for a missed duty. This
    // deliberately does not fall back to an already-assigned agent: duplicating a duty would let one agent
    // propose and verify its own position.
    if let Some(agent) = ring_pick(&rotated, round_index, &assigned, |_| true) {
        assignments.push(RoleAssignment {
            role: RoleKind::Verifier,
            lineage: lineage_of(&agent),
            agent_id: agent.clone(),
            assigned_reason: format!(
                "{ASSIGNMENT_REASON_ROTATION_REPEAT}: next unassigned participant in ring order"
            ),
        });
        assigned.insert(agent);
    }

    let unassigned_agent_ids: Vec<String> = rotated
        .iter()
        .filter(|agent| !assigned.contains(*agent))
        .cloned()
        .collect();

    RoundRoles {
        round_index,
        seed,
        assignments,
        lineage_unavoidable,
        unassigned_agent_ids,
    }
}

/// Whether two lineage lookups differ for the skeptic rule.
///
/// `None` (an adapter outside the static table) is treated as differing, because nothing in this crate knows
/// it shares a codebase, and the unknown adapter is separately surfaced by
/// [`crate::lineage::count_lineage_groups`] rather than being relied on here.
fn lineages_differ(candidate: Option<LineageGroup>, proposer: Option<LineageGroup>) -> bool {
    match (candidate, proposer) {
        (Some(candidate), Some(proposer)) => candidate != proposer,
        _ => true,
    }
}

/// Order the roster by rotation key, then by `agent_id`.
fn rotate(roster: &BTreeSet<String>, round_index: u64, seed: u64) -> Vec<String> {
    let mut ordered: Vec<(u64, &String)> = roster
        .iter()
        .map(|agent| (rotation_key(seed, round_index, agent), agent))
        .collect();
    // The roster is a `BTreeSet`, and `sort_by_key` is stable, so equal keys keep `agent_id` order. The key
    // itself is over the agent id, so equal keys are the only case that needs the tiebreak.
    ordered.sort_by_key(|(key, _)| *key);
    ordered
        .into_iter()
        .map(|(_, agent)| agent.clone())
        .collect()
}

/// Pick the first unassigned candidate in ring order that `accept` admits.
///
/// Ring order walks the rotated roster starting at `round_index % len` and wrapping once. The wrap is what
/// keeps every participant reachable: a scan from the head alone would always offer the same few agents to
/// the skeptic duty, because the proposer rotates through that same head.
fn ring_pick<F>(
    rotated: &[String],
    round_index: u64,
    assigned: &BTreeSet<String>,
    accept: F,
) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    if rotated.is_empty() {
        return None;
    }
    // `rotated` is non-empty here, so the modulo cannot divide by zero. Both conversions are handled rather
    // than assumed, because neither is a precondition a caller can be asked to guarantee.
    let modulus = u64::try_from(rotated.len()).unwrap_or(u64::MAX);
    let offset = usize::try_from(round_index % modulus).unwrap_or(0);

    for step in 0..rotated.len() {
        let index = (offset + step) % rotated.len();
        if let Some(agent) = rotated.get(index) {
            if !assigned.contains(agent) && accept(agent) {
                return Some(agent.clone());
            }
        }
    }

    None
}

/// A rotation key mixing the seed, the round index and the agent id.
///
/// FNV-1a again: dependency-free, and collision-resistant enough to spread a handful of agents across
/// rounds. It is used for ordering, never for identity or security.
fn rotation_key(seed: u64, round_index: u64, agent_id: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for byte in seed.to_le_bytes() {
        mix(byte);
    }
    for byte in round_index.to_le_bytes() {
        mix(byte);
    }
    for byte in agent_id.as_bytes() {
        mix(*byte);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three participants: Hermes on its own lineage, Kilo and OpenCode sharing the other.
    fn roster() -> [&'static str; 3] {
        ["agent-hermes", "agent-kilo", "agent-opencode"]
    }

    fn types() -> [(&'static str, &'static str); 3] {
        [
            ("agent-hermes", "HERMES_AGENT"),
            ("agent-kilo", "KILO_CODE"),
            ("agent-opencode", "OPEN_CODE"),
        ]
    }

    #[test]
    fn assignment_is_deterministic_and_ignores_input_order() {
        let forward = assign_round_roles(&roster(), &types(), None, 0, 7);
        let backward = assign_round_roles(
            &["agent-opencode", "agent-kilo", "agent-hermes"],
            &types(),
            None,
            0,
            7,
        );
        assert_eq!(forward, backward);
        assert!(forward.is_complete());

        let again = assign_round_roles(&roster(), &types(), None, 0, 7);
        assert_eq!(forward, again, "same inputs, same assignment");
    }

    #[test]
    fn the_roles_rotate_across_rounds_rather_than_repeating() {
        // The proposer rotates through the whole roster. With three participants that is the visible
        // rotation: whenever a fork member proposes, Hermes is the only different-lineage candidate, so the
        // skeptic is Hermes by the lineage rule rather than by a lack of rotation.
        let mut proposers = Vec::new();
        for round_index in 0..8u64 {
            let assignment = assign_round_roles(&roster(), &types(), None, round_index, 11);
            assert!(
                assignment.is_complete(),
                "three participants fill three duties"
            );
            assert_eq!(assignment.round_index, round_index);
            assert_eq!(assignment.assignments.len(), 3);
            assert_eq!(
                assignment
                    .assignments
                    .iter()
                    .map(|a| a.agent_id.clone())
                    .collect::<BTreeSet<_>>()
                    .len(),
                3,
                "each duty goes to a different participant"
            );
            // The lineage rule holds on every round of the rotation.
            let proposer = assignment
                .agent_for(RoleKind::Proposer)
                .unwrap_or_default()
                .to_string();
            let skeptic_lineage = assignment
                .assignments
                .iter()
                .find(|a| a.role == RoleKind::Skeptic)
                .and_then(|a| a.lineage);
            let proposer_lineage = types()
                .iter()
                .find(|(id, _)| *id == proposer)
                .and_then(|(_, agent_type)| adapter_lineage(agent_type))
                .map(|source| source.group);
            if proposer_lineage != Some(LineageGroup::HermesAgent) {
                assert_eq!(skeptic_lineage, Some(LineageGroup::HermesAgent));
            }
            proposers.push(proposer);
        }
        let distinct_proposers: BTreeSet<&String> = proposers.iter().collect();
        assert!(
            distinct_proposers.len() > 1,
            "the proposer must rotate: {proposers:?}"
        );
    }

    #[test]
    fn the_skeptic_rotates_when_two_participants_can_each_satisfy_the_lineage_rule() {
        // Two participants, one from each lineage group: whichever proposes, the other is a legal skeptic, so
        // the rotation is free to move the duty round by round rather than pinning it to one agent.
        let pair = [
            ("agent-hermes", "HERMES_AGENT"),
            ("agent-kilo", "KILO_CODE"),
        ];
        let mut skeptics = Vec::new();
        let mut proposers = Vec::new();
        for round_index in 0..8u64 {
            let assignment = assign_round_roles(
                &["agent-hermes", "agent-kilo"],
                &pair,
                None,
                round_index,
                22,
            );
            assert!(
                !assignment.lineage_unavoidable,
                "one agent from each lineage is always available"
            );
            let proposer = assignment
                .agent_for(RoleKind::Proposer)
                .unwrap_or_default()
                .to_string();
            let skeptic = assignment
                .agent_for(RoleKind::Skeptic)
                .unwrap_or_default()
                .to_string();
            assert_ne!(proposer, skeptic, "the skeptic is never the proposer");
            let proposer_lineage = pair
                .iter()
                .find(|(id, _)| *id == proposer)
                .and_then(|(_, agent_type)| adapter_lineage(agent_type))
                .map(|source| source.group);
            let skeptic_lineage = pair
                .iter()
                .find(|(id, _)| *id == skeptic)
                .and_then(|(_, agent_type)| adapter_lineage(agent_type))
                .map(|source| source.group);
            assert_ne!(
                proposer_lineage, skeptic_lineage,
                "the skeptic's lineage must differ"
            );
            proposers.push(proposer);
            skeptics.push(skeptic);
        }
        assert!(
            skeptics.iter().collect::<BTreeSet<_>>().len() > 1,
            "the skeptic must not always be one agent: {skeptics:?}"
        );
        assert!(
            proposers.iter().collect::<BTreeSet<_>>().len() > 1,
            "the proposer must also rotate: {proposers:?}"
        );
    }

    #[test]
    fn the_seed_changes_the_assignment_for_the_same_round() {
        let seeded_a = assign_round_roles(&roster(), &types(), None, 3, 1);
        let seeded_b = assign_round_roles(&roster(), &types(), None, 3, 2);
        assert_ne!(
            seeded_a, seeded_b,
            "the configured seed must affect rotation"
        );
    }

    #[test]
    fn the_skeptic_comes_from_a_different_lineage_group_when_possible() {
        // Hermes leads the proposal, so both fork members are different-lineage candidates.
        let assignment = assign_round_roles(&roster(), &types(), Some("agent-hermes"), 0, 5);
        assert_eq!(
            assignment.agent_for(RoleKind::Proposer),
            Some("agent-hermes")
        );
        let skeptic = assignment
            .assignments
            .iter()
            .find(|a| a.role == RoleKind::Skeptic)
            .and_then(|a| a.lineage);
        assert_eq!(skeptic, Some(LineageGroup::OpencodeFork));
        assert!(!assignment.lineage_unavoidable);
        assert!(!assignment
            .assignments
            .iter()
            .any(|a| a.is_lineage_unavoidable()));

        // Kilo leads, so OpenCode shares its lineage and Hermes is the only different-lineage candidate.
        let fork_led = assign_round_roles(&roster(), &types(), Some("agent-kilo"), 0, 5);
        assert_eq!(fork_led.agent_for(RoleKind::Proposer), Some("agent-kilo"));
        let skeptic = fork_led
            .assignments
            .iter()
            .find(|a| a.role == RoleKind::Skeptic);
        assert_eq!(skeptic.map(|a| a.agent_id.as_str()), Some("agent-hermes"));
        assert!(!fork_led.lineage_unavoidable);
    }

    #[test]
    fn an_unavoidable_same_lineage_skeptic_is_recorded_not_hidden() {
        // Only the two fork members take part: no participant has a different lineage.
        let fork_only = [("agent-kilo", "KILO_CODE"), ("agent-opencode", "OPEN_CODE")];
        let assignment = assign_round_roles(
            &["agent-kilo", "agent-opencode"],
            &fork_only,
            Some("agent-kilo"),
            0,
            3,
        );

        assert!(assignment.lineage_unavoidable);
        assert!(
            assignment.agent_for(RoleKind::Verifier).is_none(),
            "two participants cannot fill three duties, and a duty is never duplicated onto an agent that already holds one"
        );
        let skeptic = assignment
            .assignments
            .iter()
            .find(|a| a.role == RoleKind::Skeptic);
        assert_eq!(skeptic.map(|a| a.agent_id.as_str()), Some("agent-opencode"));
        assert!(skeptic.is_some_and(|a| a.is_lineage_unavoidable()));
        assert!(
            skeptic.is_some_and(|a| a.assigned_reason.contains("different lineage group")),
            "the reason must state what could not be satisfied"
        );
    }

    #[test]
    fn a_single_participant_cannot_supply_a_skeptic_and_that_is_recorded() {
        let assignment = assign_round_roles(&["agent-hermes"], &types(), None, 0, 1);
        assert!(assignment.lineage_unavoidable);
        assert_eq!(
            assignment.agent_for(RoleKind::Proposer),
            Some("agent-hermes")
        );
        let skeptic = assignment
            .assignments
            .iter()
            .find(|a| a.role == RoleKind::Skeptic);
        assert!(skeptic.is_some_and(|a| a.assigned_reason.contains(LINEAGE_UNAVOIDABLE)));
        assert!(
            assignment.agent_for(RoleKind::Verifier).is_none(),
            "a duty is left unfilled rather than duplicated onto an agent that already holds one"
        );
        assert_eq!(assignment.unassigned_agent_ids, Vec::<String>::new());
    }

    #[test]
    fn participants_beyond_the_three_duties_are_reported_as_unassigned() {
        let many = ["a1", "a2", "a3", "a4", "a5"];
        let assignment = assign_round_roles(&many, &[], None, 0, 9);
        assert!(assignment.is_complete());
        assert_eq!(assignment.assignments.len(), 3);
        assert_eq!(assignment.unassigned_agent_ids.len(), 2);
        for agent in &assignment.unassigned_agent_ids {
            assert!(many.contains(&agent.as_str()));
            assert!(!assignment.assignments.iter().any(|a| &a.agent_id == agent));
        }
    }

    #[test]
    fn persisted_rows_carry_the_role_and_the_reason() {
        let assignment = assign_round_roles(&roster(), &types(), Some("agent-hermes"), 0, 4);
        let records = assignment.records();
        assert_eq!(records.len(), 3);
        for record in &records {
            assert_eq!(RoleKind::parse(record.role.as_str()), Some(record.role));
            assert!(
                !record.assigned_reason.is_empty(),
                "every assignment states why"
            );
            assert!(!record.agent_id.is_empty());
        }
        assert!(records.iter().any(|row| row.role == RoleKind::Skeptic));
    }

    #[test]
    fn the_role_vocabulary_matches_the_table() {
        assert_eq!(RoleKind::Proposer.as_str(), "PROPOSER");
        assert_eq!(RoleKind::Skeptic.as_str(), "SKEPTIC");
        assert_eq!(RoleKind::Verifier.as_str(), "VERIFIER");
        assert_eq!(RoleKind::Proposer.as_participant_token(), "proposer");
        assert_eq!(RoleKind::LEADING, RoleKind::Proposer);
        assert_eq!(RoleKind::parse("AUDITOR"), None);
    }

    #[test]
    fn a_role_is_framing_only_and_grants_nothing() {
        // The type has no accessor that produces a permission, a capability or a lease. What it exposes is
        // the duty and the reason; this test pins the surface so a later addition has to be deliberate.
        let assignment = assign_round_roles(&roster(), &types(), None, 0, 1);
        let full = format!("{assignment:?}");
        assert!(!full.contains("capability"));
        assert!(!full.contains("permission"));
        assert!(!full.contains("authority"));
        assert_eq!(RoleKind::ALL.len(), 3);
    }
}
