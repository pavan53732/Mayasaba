//! Decision outcome tracking (DEC-052 item 8, `COUNCIL-ENGINE.md` "Outcome tracking").
//!
//! An outcome record says whether a decision later held, was amended, was reversed or stayed unresolved. It
//! is derived **only** from controller facts - a validation result, the `reopen_decision` command, or decision
//! supersession - so nothing in this module accepts an agent's assertion that a decision held.
//!
//! Three properties are structural rather than enforced by convention:
//!
//! * **Append-only by construction.** [`OutcomeStore`] has no update and no delete, and neither does
//!   `mayasaba-storage`: the outcome tables have no `UPDATE` or `DELETE` path anywhere in the workspace. A
//!   superseding outcome appends a new record carrying `supersedes_outcome_id`, which is why the record's
//!   `status` can move from `HELD` to `REVERSED` without any row being modified.
//! * **`HELD` requires validation evidence.** It is the only status asserting the decision survived, so
//!   [`OutcomeRecordBuilder::build`] refuses it without a validation evidence reference, and
//!   `council_decision_outcomes` carries the same rule as a `CHECK` constraint.
//! * **Informational only.** No function in this crate reads outcome data to decide a mode, a threshold, a
//!   route or an authority. There is deliberately no `select_mode` overload that takes an outcome, and the
//!   mode selector has no import of this module. DEC-052 records that using outcome tracking to influence
//!   routing or thresholds would be a hidden second authority requiring its own decision.

use crate::clock::Clock;
use crate::error::{CouncilError, Result};
use crate::mode::{CouncilMode, DecisionClass};

/// Whether a decision held, was amended, was reversed, or stayed unresolved.
///
/// The four-value vocabulary is fixed by DEC-033 and `decision-outcome.schema.json`; nothing here adds a
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutcomeStatus {
    /// The decision survived validation and the record carries the evidence.
    Held,
    /// The decision stands with a change.
    Amended,
    /// The decision was undone.
    Reversed,
    /// No controller fact has resolved the decision yet.
    Unresolved,
}

impl OutcomeStatus {
    /// Every status, ordered strongest claim first.
    pub const ALL: [OutcomeStatus; 4] = [
        OutcomeStatus::Held,
        OutcomeStatus::Amended,
        OutcomeStatus::Reversed,
        OutcomeStatus::Unresolved,
    ];

    /// The contract spelling, as stored in `council_decision_outcomes.status`.
    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeStatus::Held => "HELD",
            OutcomeStatus::Amended => "AMENDED",
            OutcomeStatus::Reversed => "REVERSED",
            OutcomeStatus::Unresolved => "UNRESOLVED",
        }
    }

    /// Parse the contract spelling.
    pub fn parse(value: &str) -> Option<Self> {
        OutcomeStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    /// Whether this status asserts the decision survived, and therefore requires validation evidence.
    pub fn requires_validation_evidence(self) -> bool {
        self == OutcomeStatus::Held
    }

    /// Whether this status describes a change to an earlier record, and therefore requires the id of the
    /// record it supersedes.
    pub fn requires_superseded_outcome(self) -> bool {
        matches!(self, OutcomeStatus::Amended | OutcomeStatus::Reversed)
    }
}

impl std::fmt::Display for OutcomeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The controller fact an outcome was derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeSource {
    /// A validation run's result.
    ValidationResult,
    /// The `reopen_decision` command.
    ReopenDecision,
    /// Decision supersession.
    UserSupersession,
}

impl OutcomeSource {
    /// Every source.
    pub const ALL: [OutcomeSource; 3] = [
        OutcomeSource::ValidationResult,
        OutcomeSource::ReopenDecision,
        OutcomeSource::UserSupersession,
    ];

    /// The contract spelling, as stored in `council_decision_outcomes.source`.
    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeSource::ValidationResult => "VALIDATION_RESULT",
            OutcomeSource::ReopenDecision => "REOPEN_DECISION",
            OutcomeSource::UserSupersession => "USER_SUPERSESSION",
        }
    }

    /// Parse the contract spelling.
    pub fn parse(value: &str) -> Option<Self> {
        OutcomeSource::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
    }
}

/// One agent's stance on a decision, as a link row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeAgentLink {
    pub agent_id: String,
    /// The position the stance came from, when the round recorded one.
    pub position_id: Option<String>,
    /// The stance, as the round recorded it. Free text by contract, so it is carried rather than re-typed.
    pub stance: String,
}

impl OutcomeAgentLink {
    /// A link with a position.
    pub fn new(agent_id: &str, position_id: &str, stance: &str) -> Self {
        OutcomeAgentLink {
            agent_id: agent_id.to_string(),
            position_id: Some(position_id.to_string()),
            stance: stance.to_string(),
        }
    }

    /// A link with no position, for a stance the round did not tie to one.
    pub fn without_position(agent_id: &str, stance: &str) -> Self {
        OutcomeAgentLink {
            agent_id: agent_id.to_string(),
            position_id: None,
            stance: stance.to_string(),
        }
    }
}

/// An append-only outcome record, as `decision-outcome.schema.json` validates it.
///
/// `informational_only` is pinned to `true` by the contract, so it is not a field: a record that could carry
/// `false` would be a record claiming a routing authority this feature deliberately does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilDecisionOutcome {
    pub outcome_record_id: String,
    pub decision_id: String,
    pub council_session_id: Option<String>,
    pub round_id: Option<String>,
    pub mode: CouncilMode,
    pub decision_class: DecisionClass,
    pub status: OutcomeStatus,
    /// Required by [`OutcomeStatus::Held`], absent otherwise.
    pub validation_evidence_id: Option<String>,
    pub source: OutcomeSource,
    /// The record this one supersedes, which is how an amendment or reversal stays append-only.
    pub supersedes_outcome_id: Option<String>,
    pub recorded_at: String,
    pub agent_links: Vec<OutcomeAgentLink>,
}

impl CouncilDecisionOutcome {
    /// Whether this record is informational only.
    ///
    /// Always `true`. The accessor exists so a caller reading the record has the contract's field without
    /// this crate modelling a value that cannot vary.
    pub fn informational_only(&self) -> bool {
        true
    }

    /// Whether this record asserts the decision survived.
    pub fn asserts_held(&self) -> bool {
        self.status == OutcomeStatus::Held
    }
}

/// Builds a [`CouncilDecisionOutcome`].
///
/// The builder exists so the contract's conditionals are checked in one place, before a caller can reach
/// storage with a record that would be rejected there. There is no setter for a status after construction
/// and no `update`: a change is a new record that supersedes the old one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeRecordBuilder {
    outcome_record_id: String,
    decision_id: String,
    council_session_id: Option<String>,
    round_id: Option<String>,
    mode: CouncilMode,
    decision_class: DecisionClass,
    status: OutcomeStatus,
    validation_evidence_id: Option<String>,
    source: OutcomeSource,
    supersedes_outcome_id: Option<String>,
    agent_links: Vec<OutcomeAgentLink>,
}

impl OutcomeRecordBuilder {
    /// Start a record for `decision_id`.
    pub fn new(
        outcome_record_id: &str,
        decision_id: &str,
        mode: CouncilMode,
        decision_class: DecisionClass,
        status: OutcomeStatus,
        source: OutcomeSource,
    ) -> Self {
        OutcomeRecordBuilder {
            outcome_record_id: outcome_record_id.to_string(),
            decision_id: decision_id.to_string(),
            council_session_id: None,
            round_id: None,
            mode,
            decision_class,
            status,
            validation_evidence_id: None,
            source,
            supersedes_outcome_id: None,
            agent_links: Vec::new(),
        }
    }

    /// Scope the record to the council session that produced the decision.
    pub fn in_session(mut self, council_session_id: &str) -> Self {
        self.council_session_id = Some(council_session_id.to_string());
        self
    }

    /// Scope the record to the round that produced the decision.
    pub fn in_round(mut self, round_id: &str) -> Self {
        self.round_id = Some(round_id.to_string());
        self
    }

    /// Attach the validation evidence that supports a `HELD` record.
    pub fn validated_by(mut self, validation_evidence_id: &str) -> Self {
        self.validation_evidence_id = Some(validation_evidence_id.to_string());
        self
    }

    /// Name the record this one supersedes.
    pub fn superseding(mut self, supersedes_outcome_id: &str) -> Self {
        self.supersedes_outcome_id = Some(supersedes_outcome_id.to_string());
        self
    }

    /// Add one agent link.
    pub fn with_link(mut self, link: OutcomeAgentLink) -> Self {
        self.agent_links.push(link);
        self
    }

    /// Add several agent links.
    pub fn with_links(mut self, links: &[OutcomeAgentLink]) -> Self {
        self.agent_links.extend(links.iter().cloned());
        self
    }

    /// Validate and build, stamping `recorded_at` from the injected clock.
    ///
    /// Refuses: a `HELD` record with no validation evidence reference, an `AMENDED` or `REVERSED` record with
    /// no superseded record id, and a link that names no agent. The first is required by DEC-052 and the
    /// record contract; the second and third are this builder's reading of the same append-only rule, stated
    /// because the contract's `allOf` covers `HELD` alone.
    pub fn build(self, clock: &dyn Clock) -> Result<CouncilDecisionOutcome> {
        if self.status.requires_validation_evidence() && self.validation_evidence_id.is_none() {
            return Err(CouncilError::contract(
                self.status.as_str(),
                CouncilError::CONTRACT_CODE,
            ));
        }
        if self.status.requires_superseded_outcome() && self.supersedes_outcome_id.is_none() {
            return Err(CouncilError::supersession(
                self.status.as_str(),
                CouncilError::CONTRACT_CODE,
            ));
        }
        for (index, link) in self.agent_links.iter().enumerate() {
            if link.agent_id.trim().is_empty() {
                return Err(CouncilError::link(index, CouncilError::CONTRACT_CODE));
            }
        }

        Ok(CouncilDecisionOutcome {
            outcome_record_id: self.outcome_record_id,
            decision_id: self.decision_id,
            council_session_id: self.council_session_id,
            round_id: self.round_id,
            mode: self.mode,
            decision_class: self.decision_class,
            status: self.status,
            validation_evidence_id: self.validation_evidence_id,
            source: self.source,
            supersedes_outcome_id: self.supersedes_outcome_id,
            recorded_at: clock.now_rfc3339(),
            agent_links: self.agent_links,
        })
    }
}

impl CouncilDecisionOutcome {
    /// The storage request for this record.
    pub fn to_storage(&self) -> mayasaba_storage::NewCouncilDecisionOutcome {
        mayasaba_storage::NewCouncilDecisionOutcome {
            outcome_record_id: self.outcome_record_id.clone(),
            decision_id: self.decision_id.clone(),
            council_session_id: self.council_session_id.clone(),
            round_id: self.round_id.clone(),
            mode: self.mode.as_str().to_string(),
            decision_class: self.decision_class.as_str().to_string(),
            status: self.status.as_str().to_string(),
            validation_evidence_id: self.validation_evidence_id.clone(),
            source: self.source.as_str().to_string(),
            supersedes_outcome_id: self.supersedes_outcome_id.clone(),
            recorded_at: self.recorded_at.clone(),
            agent_links: self
                .agent_links
                .iter()
                .map(|link| mayasaba_storage::NewCouncilOutcomeAgentLink {
                    agent_id: link.agent_id.clone(),
                    position_id: link.position_id.clone(),
                    stance: link.stance.clone(),
                })
                .collect(),
        }
    }
}

/// An append-only collection of outcome records.
///
/// Every operation that adds takes `self` by value and returns the collection that includes the addition, so
/// there is no way to remove, replace or reorder a record through this type. There is deliberately no
/// `records_mut`, no `remove`, no `replace`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutcomeStore {
    records: Vec<CouncilDecisionOutcome>,
}

impl OutcomeStore {
    /// An empty store.
    pub fn new() -> Self {
        OutcomeStore {
            records: Vec::new(),
        }
    }

    /// Every record, oldest first.
    pub fn records(&self) -> &[CouncilDecisionOutcome] {
        &self.records
    }

    /// How many records the store holds.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the store holds nothing.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Append a record, returning the store that includes it.
    ///
    /// The only way to see a changed status for a decision is to append a record that supersedes the old one.
    pub fn appended(mut self, record: CouncilDecisionOutcome) -> Self {
        self.records.push(record);
        self
    }

    /// The newest record for `decision_id`, or `None` when no outcome has been recorded for it.
    ///
    /// Ordering is by `recorded_at` then by `outcome_record_id`, so a same-instant append still resolves
    /// deterministically. This reads records; it never decides anything with them.
    pub fn latest_for(&self, decision_id: &str) -> Option<&CouncilDecisionOutcome> {
        self.records
            .iter()
            .filter(|record| record.decision_id == decision_id)
            .max_by(|left, right| {
                left.recorded_at
                    .cmp(&right.recorded_at)
                    .then_with(|| left.outcome_record_id.cmp(&right.outcome_record_id))
            })
    }

    /// Every record for one decision, oldest first.
    pub fn for_decision(&self, decision_id: &str) -> Vec<&CouncilDecisionOutcome> {
        let mut matching: Vec<&CouncilDecisionOutcome> = self
            .records
            .iter()
            .filter(|record| record.decision_id == decision_id)
            .collect();
        matching.sort_by(|left, right| {
            left.recorded_at
                .cmp(&right.recorded_at)
                .then_with(|| left.outcome_record_id.cmp(&right.outcome_record_id))
        });
        matching
    }

    /// How many decisions carry each status, for reporting.
    ///
    /// Counts, not rates: `council-policies.json:reporting` requires raw counts with a sample size and no
    /// percentage below `minimum_sample_for_percentage`, and this returns the raw material for that rather
    /// than a derived figure.
    pub fn status_counts(&self) -> Vec<(OutcomeStatus, usize)> {
        let mut counts: Vec<(OutcomeStatus, usize)> = OutcomeStatus::ALL
            .iter()
            .map(|status| (*status, 0))
            .collect();
        for record in &self.records {
            for entry in counts.iter_mut() {
                if entry.0 == record.status {
                    entry.1 = entry.1.saturating_add(1);
                }
            }
        }
        counts
    }

    /// Whether any record for `decision_id` asserts the decision held.
    ///
    /// Reported for a caller that displays decision history. It is **not** consulted by mode selection or by
    /// any other decision in this crate.
    pub fn is_held(&self, decision_id: &str) -> bool {
        self.latest_for(decision_id)
            .is_some_and(CouncilDecisionOutcome::asserts_held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::new("2026-10-04T00:00:00Z")
    }

    fn held(id: &str, evidence: Option<&str>) -> Result<CouncilDecisionOutcome> {
        let builder = OutcomeRecordBuilder::new(
            id,
            "dec_1",
            CouncilMode::Review,
            DecisionClass::Architecture,
            OutcomeStatus::Held,
            OutcomeSource::ValidationResult,
        );
        match evidence {
            Some(evidence_id) => builder.validated_by(evidence_id).build(&clock()),
            None => builder.build(&clock()),
        }
    }

    #[test]
    fn held_is_refused_without_a_validation_evidence_reference() {
        let refused = held("out_1", None);
        assert!(refused.is_err());
        match refused {
            Err(error) => {
                assert_eq!(error.code(), "SCHEMA_INVALID");
                assert_eq!(error.gap(), Some("OUTCOME_EVIDENCE_REQUIRED"));
                let text = error.to_string();
                assert!(
                    text.contains("HELD"),
                    "the refusal must name the status: {text}"
                );
                assert!(text.contains("validation evidence"));
            }
            Ok(record) => assert!(
                record.asserts_held(),
                "the record was expected to be refused"
            ),
        }

        // With evidence it is accepted, and the evidence reference survives onto the record.
        let accepted = held("out_1", Some("ev_1"));
        assert!(accepted.is_ok());
        match &accepted {
            Ok(record) => {
                assert_eq!(record.validation_evidence_id.as_deref(), Some("ev_1"));
                assert_eq!(record.status, OutcomeStatus::Held);
                assert_eq!(record.recorded_at, "2026-10-04T00:00:00Z");
                assert!(record.informational_only());
            }
            Err(error) => assert!(error.code().is_empty(), "build failed unexpectedly"),
        }
    }

    #[test]
    fn an_amendment_or_reversal_must_name_the_record_it_supersedes() {
        for status in [OutcomeStatus::Amended, OutcomeStatus::Reversed] {
            let refused = OutcomeRecordBuilder::new(
                "out_2",
                "dec_1",
                CouncilMode::Full,
                DecisionClass::Security,
                status,
                OutcomeSource::ReopenDecision,
            )
            .build(&clock());
            assert!(refused.is_err());

            let accepted = OutcomeRecordBuilder::new(
                "out_2",
                "dec_1",
                CouncilMode::Full,
                DecisionClass::Security,
                status,
                OutcomeSource::ReopenDecision,
            )
            .superseding("out_1")
            .build(&clock());
            assert!(accepted.is_ok());
        }
    }

    #[test]
    fn an_unresolved_record_needs_no_evidence() {
        let record = OutcomeRecordBuilder::new(
            "out_3",
            "dec_1",
            CouncilMode::Solo,
            DecisionClass::Routine,
            OutcomeStatus::Unresolved,
            OutcomeSource::ValidationResult,
        )
        .build(&clock());
        assert!(record.is_ok());
    }

    #[test]
    fn an_agent_link_must_name_an_agent() {
        let refused = OutcomeRecordBuilder::new(
            "out_4",
            "dec_1",
            CouncilMode::Review,
            DecisionClass::Architecture,
            OutcomeStatus::Unresolved,
            OutcomeSource::ValidationResult,
        )
        .with_link(OutcomeAgentLink::without_position("  ", "AGREED"))
        .build(&clock());
        assert!(refused.is_err());

        let accepted = OutcomeRecordBuilder::new(
            "out_4",
            "dec_1",
            CouncilMode::Review,
            DecisionClass::Architecture,
            OutcomeStatus::Unresolved,
            OutcomeSource::ValidationResult,
        )
        .with_links(&[
            OutcomeAgentLink::new("agent-hermes", "pos_1", "AGREED"),
            OutcomeAgentLink::without_position("agent-kilo", "DISSENTED"),
        ])
        .build(&clock());
        assert!(accepted.is_ok());
        match &accepted {
            Ok(record) => {
                assert_eq!(record.agent_links.len(), 2);
                assert_eq!(record.agent_links[0].position_id.as_deref(), Some("pos_1"));
                assert_eq!(record.agent_links[1].position_id, None);
            }
            Err(error) => assert!(error.code().is_empty(), "build failed unexpectedly"),
        }
    }

    #[test]
    fn the_store_is_append_only_and_supersession_appends() {
        let first = held("out_1", Some("ev_1"));
        let store = match first {
            Ok(record) => OutcomeStore::new().appended(record),
            Err(_) => OutcomeStore::new(),
        };
        assert_eq!(store.len(), 1);
        assert!(store.is_held("dec_1"));

        let reversal = OutcomeRecordBuilder::new(
            "out_2",
            "dec_1",
            CouncilMode::Review,
            DecisionClass::Architecture,
            OutcomeStatus::Reversed,
            OutcomeSource::ReopenDecision,
        )
        .superseding("out_1")
        .build(&clock());
        let store = match reversal {
            Ok(record) => store.clone().appended(record),
            Err(_) => store.clone(),
        };

        assert_eq!(store.len(), 2, "a reversal appends; nothing is replaced");
        assert!(
            !store.is_held("dec_1"),
            "the newest record decides what the store reports"
        );
        assert_eq!(
            store.records()[0].status,
            OutcomeStatus::Held,
            "history is preserved"
        );
        assert_eq!(store.records()[1].status, OutcomeStatus::Reversed);
        assert_eq!(store.for_decision("dec_1").len(), 2);
        assert_eq!(
            store
                .latest_for("dec_1")
                .map(|record| record.outcome_record_id.as_str()),
            Some("out_2")
        );
        assert_eq!(store.latest_for("dec_missing"), None);
    }

    #[test]
    fn the_vocabularies_are_exactly_the_contracts() {
        assert_eq!(OutcomeStatus::ALL.len(), 4);
        for status in OutcomeStatus::ALL {
            assert_eq!(OutcomeStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(OutcomeStatus::parse("SUPERSEDED"), None);
        assert!(OutcomeStatus::Held.requires_validation_evidence());
        assert!(!OutcomeStatus::Amended.requires_validation_evidence());
        assert!(OutcomeStatus::Amended.requires_superseded_outcome());
        assert!(OutcomeStatus::Reversed.requires_superseded_outcome());
        assert!(!OutcomeStatus::Unresolved.requires_superseded_outcome());

        assert_eq!(OutcomeSource::ALL.len(), 3);
        for source in OutcomeSource::ALL {
            assert_eq!(OutcomeSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(OutcomeSource::parse("AGENT_CLAIM"), None);
    }

    #[test]
    fn status_counts_are_raw_counts_not_rates() {
        let store = OutcomeStore::new();
        let store = match held("out_1", Some("ev_1")) {
            Ok(record) => store.appended(record),
            Err(_) => store,
        };
        let store = match held("out_2", Some("ev_2")) {
            Ok(record) => store.appended(record),
            Err(_) => store,
        };
        let counts = store.status_counts();
        let held_count = counts
            .iter()
            .find(|(status, _)| *status == OutcomeStatus::Held)
            .map(|(_, count)| *count);
        assert_eq!(held_count, Some(2));
        assert_eq!(
            counts.len(),
            OutcomeStatus::ALL.len(),
            "every status is reported, including zeroes"
        );
    }
}
