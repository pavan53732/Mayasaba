//! Lineage-group corroboration (DEC-052 item 4, `COUNCIL-ENGINE.md` "Lineage-group corroboration").
//!
//! Corroboration counts independent *implementations*, not agents and not models. `ARCHITECTURE.md`
//! section 2 states the independence caveat plainly:
//!
//! > Kilo Code CLI is a fork of OpenCode CLI and shares its codebase lineage, config surface and much of
//! > its permission model. Do not treat Kilo and OpenCode as independent corroboration of each other in
//! > council deliberation: agreement between them is weaker evidence than agreement between either and
//! > Hermes.
//!
//! DEC-029 fixes the delivered adapter set at exactly three CLIs, so the mapping below is total over the
//! adapters the product ships. It is a static table precisely because lineage must come from static adapter
//! facts: an agent's own claim about which codebase it descends from is self-report, and self-report is
//! never read here.
//!
//! The two facts this module exists to get right, both declared in exactly one place - [`ADAPTER_LINEAGE`]:
//!
//! * `KILO_CODE` and `OPEN_CODE` are **one** group, [`LineageGroup::OpencodeFork`], because Kilo is a fork
//!   of OpenCode.
//! * `HERMES_AGENT` is its **own** group, [`LineageGroup::HermesAgent`]. It is not an OpenCode fork, so
//!   Hermes plus either other adapter is two corroborations.

/// A codebase lineage: what a group of agent adapters descends from.
///
/// Two variants, because DEC-029's shipped adapter set has exactly two lineages. An adapter the static table
/// does not know has no lineage at all rather than an `Unknown` group, so it cannot be counted as an
/// independent witness: [`count_lineage_groups`] reports it in `unknown_agent_types` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LineageGroup {
    /// OpenCode CLI and its fork, Kilo Code CLI.
    OpencodeFork,
    /// Hermes Agent CLI, which is not an OpenCode fork.
    HermesAgent,
}

impl LineageGroup {
    /// Every group, in a stable order.
    pub const ALL: [LineageGroup; 2] = [LineageGroup::OpencodeFork, LineageGroup::HermesAgent];

    /// The contract token for this group.
    pub fn as_str(self) -> &'static str {
        match self {
            LineageGroup::OpencodeFork => "OPENCODE_FORK",
            LineageGroup::HermesAgent => "HERMES_AGENT",
        }
    }

    /// Parse the contract token.
    pub fn parse(value: &str) -> Option<Self> {
        LineageGroup::ALL
            .into_iter()
            .find(|group| group.as_str() == value)
    }
}

impl std::fmt::Display for LineageGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One row of the static adapter-to-lineage table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineageSource {
    /// The adapter type, as it appears in an MCF-v2 identity and in `agents.agent_type`.
    pub agent_type: &'static str,
    /// The lineage the adapter descends from.
    pub group: LineageGroup,
    /// Whether this adapter shares a codebase with another adapter in the table.
    ///
    /// Recorded per row rather than derived from "is another row's group equal", so the fork relationship
    /// is a stated fact about one adapter and not an inference from a grouping that a later edit could
    /// accidentally change.
    pub shares_codebase: bool,
}

/// The static adapter-to-lineage table. The single declaration of the fork relationship in code.
///
/// This is deliberately **not** configuration. Lineage is a fact about a codebase, and a configurable
/// lineage would let a user - or an agent through a mis-scoped config key - manufacture independence that
/// does not exist. `council-policies.json` configures the *minimum group count*
/// (`minimum_lineage_groups_for_corroboration`), never the mapping.
pub const ADAPTER_LINEAGE: &[LineageSource] = &[
    LineageSource {
        agent_type: "HERMES_AGENT",
        group: LineageGroup::HermesAgent,
        shares_codebase: false,
    },
    LineageSource {
        agent_type: "KILO_CODE",
        group: LineageGroup::OpencodeFork,
        shares_codebase: true,
    },
    LineageSource {
        agent_type: "OPEN_CODE",
        group: LineageGroup::OpencodeFork,
        shares_codebase: true,
    },
];

/// The default minimum lineage groups, matching `council-policies.json:evidence`.
pub const DEFAULT_MINIMUM_LINEAGE_GROUPS: usize = 2;

/// The lineage of one adapter type, or `None` when the static table does not know it.
pub fn adapter_lineage(agent_type: &str) -> Option<&'static LineageSource> {
    ADAPTER_LINEAGE
        .iter()
        .find(|entry| entry.agent_type == agent_type)
}

/// Whether `left` and `right` share a codebase, and therefore corroborate each other only once.
///
/// Two adapter types that are both unknown to the table are *not* assumed to share a codebase: nothing here
/// knows them, and assuming shared lineage would discount a genuinely independent future adapter.
pub fn share_codebase(left: &str, right: &str) -> bool {
    match (adapter_lineage(left), adapter_lineage(right)) {
        (Some(left), Some(right)) => left.group == right.group,
        _ => false,
    }
}

/// The distinct lineage groups a roster represents, in a stable order, ignoring unknown adapters.
pub fn lineage_groups_for(agent_types: &[&str]) -> Vec<LineageGroup> {
    let mut groups: Vec<LineageGroup> = agent_types
        .iter()
        .filter_map(|agent_type| adapter_lineage(agent_type))
        .map(|entry| entry.group)
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

/// The outcome of counting lineage groups against a configured minimum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corroboration {
    /// How many distinct lineage groups the roster represents.
    pub group_count: usize,
    /// The configured minimum. `council-policies.json:evidence.minimum_lineage_groups_for_corroboration`
    /// ships `2`.
    pub minimum_groups: usize,
    /// The distinct groups, in a stable order.
    pub groups: Vec<LineageGroup>,
    /// Adapter types that were not in the static table, so a caller can surface an adapter this build does
    /// not know about instead of silently counting it as independent.
    pub unknown_agent_types: Vec<String>,
}

impl Corroboration {
    /// Whether the roster reaches the configured number of independent lineages.
    pub fn is_corroborated(&self) -> bool {
        self.group_count >= self.minimum_groups
    }

    /// Whether the roster is below the minimum and must be recorded as uncorroborated.
    pub fn is_uncorroborated(&self) -> bool {
        !self.is_corroborated()
    }

    /// Whether any participating adapter is absent from the static lineage table.
    pub fn has_unknown_lineage(&self) -> bool {
        !self.unknown_agent_types.is_empty()
    }
}

/// Count distinct lineage groups among `agent_types`.
///
/// `minimum_groups` is configuration; [`DEFAULT_MINIMUM_LINEAGE_GROUPS`] is the shipped value. A count below
/// the minimum is reported as uncorroborated - a recorded quality fact, never a silent pass.
pub fn count_lineage_groups(agent_types: &[&str], minimum_groups: usize) -> Corroboration {
    let groups = lineage_groups_for(agent_types);

    let mut unknown: Vec<String> = agent_types
        .iter()
        .filter(|agent_type| adapter_lineage(agent_type).is_none())
        .map(|agent_type| (*agent_type).to_string())
        .collect();
    unknown.sort();
    unknown.dedup();

    Corroboration {
        group_count: groups.len(),
        minimum_groups,
        groups,
        unknown_agent_types: unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three shipped adapters, as `mayasaba_protocol` names them.
    const SHIPPED: [&str; 3] = ["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"];

    #[test]
    fn the_static_table_declares_the_fork_relationship_in_one_place() {
        assert_eq!(
            adapter_lineage("KILO_CODE").map(|entry| entry.group),
            Some(LineageGroup::OpencodeFork)
        );
        assert_eq!(
            adapter_lineage("OPEN_CODE").map(|entry| entry.group),
            Some(LineageGroup::OpencodeFork)
        );
        assert_eq!(
            adapter_lineage("HERMES_AGENT").map(|entry| entry.group),
            Some(LineageGroup::HermesAgent)
        );

        assert!(adapter_lineage("KILO_CODE").is_some_and(|entry| entry.shares_codebase));
        assert!(adapter_lineage("OPEN_CODE").is_some_and(|entry| entry.shares_codebase));
        assert!(!adapter_lineage("HERMES_AGENT").is_some_and(|entry| entry.shares_codebase));
        assert!(share_codebase("KILO_CODE", "OPEN_CODE"));
        assert!(!share_codebase("HERMES_AGENT", "KILO_CODE"));
    }

    #[test]
    fn kilo_and_opencode_count_as_one_lineage_group() {
        let corroboration =
            count_lineage_groups(&["KILO_CODE", "OPEN_CODE"], DEFAULT_MINIMUM_LINEAGE_GROUPS);
        assert_eq!(
            corroboration.group_count, 1,
            "two front ends on one codebase are one witness"
        );
        assert!(
            corroboration.is_uncorroborated(),
            "fewer than two groups is recorded as uncorroborated"
        );
        assert_eq!(corroboration.groups, vec![LineageGroup::OpencodeFork]);
    }

    #[test]
    fn hermes_plus_kilo_is_two_lineage_groups() {
        let corroboration = count_lineage_groups(
            &["HERMES_AGENT", "KILO_CODE"],
            DEFAULT_MINIMUM_LINEAGE_GROUPS,
        );
        assert_eq!(corroboration.group_count, 2);
        assert!(corroboration.is_corroborated());
        assert_eq!(
            corroboration.groups,
            vec![LineageGroup::OpencodeFork, LineageGroup::HermesAgent]
        );

        let with_opencode = count_lineage_groups(
            &["HERMES_AGENT", "OPEN_CODE"],
            DEFAULT_MINIMUM_LINEAGE_GROUPS,
        );
        assert_eq!(
            with_opencode.group_count, 2,
            "Hermes plus either fork member is two groups"
        );
    }

    #[test]
    fn all_three_shipped_adapters_are_two_groups() {
        let all = count_lineage_groups(&SHIPPED, DEFAULT_MINIMUM_LINEAGE_GROUPS);
        assert_eq!(
            all.group_count, 2,
            "three agents, two independent implementations"
        );
        assert!(all.is_corroborated());
        assert!(!all.has_unknown_lineage());
    }

    #[test]
    fn the_minimum_is_configuration() {
        let one = count_lineage_groups(&["HERMES_AGENT", "KILO_CODE"], 1);
        assert!(one.is_corroborated());
        assert_eq!(one.minimum_groups, 1);

        let three = count_lineage_groups(&SHIPPED, 3);
        assert!(
            three.is_uncorroborated(),
            "two implementations cannot satisfy a minimum of three"
        );
        assert_eq!(three.minimum_groups, 3);
        assert_eq!(three.group_count, 2);
    }

    #[test]
    fn an_unknown_adapter_is_surfaced_and_never_collapsed_with_another() {
        let mixed = count_lineage_groups(
            &["HERMES_AGENT", "FUTURE_CLI"],
            DEFAULT_MINIMUM_LINEAGE_GROUPS,
        );
        assert_eq!(
            mixed.group_count, 1,
            "an unknown adapter contributes no known lineage group"
        );
        assert!(mixed.is_uncorroborated());
        assert!(mixed.has_unknown_lineage());
        assert_eq!(mixed.unknown_agent_types, vec!["FUTURE_CLI".to_string()]);

        let two_unknown =
            count_lineage_groups(&["FUTURE_CLI", "OTHER_CLI"], DEFAULT_MINIMUM_LINEAGE_GROUPS);
        assert_eq!(two_unknown.group_count, 0);
        assert_eq!(two_unknown.unknown_agent_types.len(), 2);
        assert!(
            !share_codebase("FUTURE_CLI", "OTHER_CLI"),
            "unknown lineage is never assumed shared"
        );
        assert!(adapter_lineage("FUTURE_CLI").is_none());
    }

    #[test]
    fn counts_are_order_independent_and_deduplicated() {
        let forward = count_lineage_groups(&["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"], 2);
        let backward = count_lineage_groups(&["OPEN_CODE", "KILO_CODE", "HERMES_AGENT"], 2);
        assert_eq!(forward, backward);
        assert_eq!(
            count_lineage_groups(&["HERMES_AGENT", "HERMES_AGENT"], 2).group_count,
            1
        );
    }

    #[test]
    fn an_empty_roster_is_uncorroborated_rather_than_an_error() {
        let none = count_lineage_groups(&[], DEFAULT_MINIMUM_LINEAGE_GROUPS);
        assert_eq!(none.group_count, 0);
        assert!(none.is_uncorroborated());
        assert_eq!(lineage_groups_for(&[]), Vec::new());
    }
}
