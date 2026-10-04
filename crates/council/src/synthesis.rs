//! Synthesis coverage and independent review (DEC-052 item 6, `COUNCIL-ENGINE.md` "Synthesis review").
//!
//! A round that closes on a chair-submitted `SYNTHESIS` has two checks here, and both are deterministic:
//!
//! * **Coverage.** The synthesis must cite every surviving position. What is checked is the synthesis
//!   position's `responds_to_position_ids` covering the surviving set, which is the existing mechanism; the
//!   omitted ids are returned so a caller can report them rather than only learning that something is
//!   missing.
//! * **Review.** A synthesis is reviewed by a non-chair participant who is not its author. The chair can
//!   never self-certify, and neither can the synthesis's own author.
//!
//! **No new message type.** The review is modelled as what it is: an existing MCF-v2 `CRITIQUE` addressing
//! the synthesis position id. Nothing here constructs a message, and there is deliberately no
//! `SynthesisReview` type - a second type would be a second vocabulary for a concept MCF-v2 already has.
//! Detected distortions belong in the escalation packet and the Control Room, which are the callers' concern.

use std::collections::BTreeSet;

use crate::lineage::LineageGroup;
use crate::roles::RoleKind;

/// The result of the coverage check on a synthesis position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisCoverage {
    /// Whether the synthesis cites every surviving position.
    pub complete: bool,
    /// Surviving position ids the synthesis does not cite, sorted, so a caller reports exactly what was
    /// omitted.
    pub omitted_position_ids: Vec<String>,
    /// How many surviving positions the synthesis does cite.
    pub covered_count: usize,
    /// How many surviving positions the round has.
    pub surviving_count: usize,
}

impl SynthesisCoverage {
    /// Whether anything was omitted.
    pub fn has_omissions(&self) -> bool {
        !self.omitted_position_ids.is_empty()
    }
}

/// How many positions a synthesis cites that are not among the survivors.
///
/// Not a failure by itself - a synthesis may legitimately reference a position that was later withdrawn - so
/// it is reported as a count rather than as a refusal.
pub fn extra_citations(synthesis: &SynthesisPosition, coverage: &SynthesisCoverage) -> usize {
    synthesis
        .responds_to
        .len()
        .saturating_sub(coverage.covered_count)
}

/// Why a check refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageGap {
    /// The synthesis position cites nothing at all.
    NoCitations,
    /// The synthesis does not cite these surviving positions.
    OmittedPositions { position_ids: Vec<String> },
    /// The synthesis cites itself, which is not a citation of a contributing position.
    SelfReferential,
}

/// A synthesis position, as far as these checks are concerned.
///
/// Only the three facts the checks read are modelled. The full position lives in `council_positions`; this
/// crate does not take ownership of the round's position model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisPosition {
    /// The `SYNTHESIS` position's own id, which is also the `CRITIQUE` target.
    pub position_id: String,
    /// The author of the position, who must not be the reviewer.
    pub author_agent_id: String,
    /// The positions the synthesis responds to, as the round recorded them.
    pub responds_to: Vec<String>,
}

impl SynthesisPosition {
    /// A synthesis position.
    pub fn new(position_id: &str, author_agent_id: &str, responds_to: &[&str]) -> Self {
        SynthesisPosition {
            position_id: position_id.to_string(),
            author_agent_id: author_agent_id.to_string(),
            responds_to: responds_to.iter().map(|id| (*id).to_string()).collect(),
        }
    }
}

/// Check that a synthesis cites every surviving position.
///
/// The surviving set is the caller's: which positions survived a round is a fact only the round holds, and
/// this crate must not infer it from the synthesis. Surviving ids are returned sorted in the omission list so
/// two runs report the same order.
pub fn check_synthesis_coverage(
    synthesis: &SynthesisPosition,
    surviving_position_ids: &[&str],
) -> Result<SynthesisCoverage, CoverageGap> {
    let cited: BTreeSet<&str> = synthesis.responds_to.iter().map(String::as_str).collect();

    if synthesis.responds_to.is_empty() {
        return Err(CoverageGap::NoCitations);
    }
    if cited.contains(synthesis.position_id.as_str()) {
        return Err(CoverageGap::SelfReferential);
    }

    // The surviving set excludes the synthesis itself: if the round lists its own synthesis among the
    // survivors, citing it is not the citation of a contributing position the rule asks for.
    let survivors: BTreeSet<String> = surviving_position_ids
        .iter()
        .filter(|id| **id != synthesis.position_id)
        .map(|id| (*id).to_string())
        .collect();

    let covered_count = survivors
        .iter()
        .filter(|id| cited.contains(id.as_str()))
        .count();
    let omitted: Vec<String> = survivors
        .iter()
        .filter(|id| !cited.contains(id.as_str()))
        .cloned()
        .collect();

    match omitted.is_empty() {
        false => Err(CoverageGap::OmittedPositions {
            position_ids: omitted,
        }),
        true => Ok(SynthesisCoverage {
            complete: true,
            omitted_position_ids: Vec::new(),
            covered_count,
            surviving_count: survivors.len(),
        }),
    }
}

/// Why a reviewer may not review a synthesis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerRefusal {
    /// The reviewer is the chair. The chair can never self-certify its own synthesis.
    Chair,
    /// The reviewer authored the synthesis.
    SynthesisAuthor {
        /// The synthesis author, echoed so the refusal is diagnosable.
        author_agent_id: String,
    },
    /// The reviewer is not a participant in the round.
    NotAParticipant,
    /// The reviewer was not named.
    UnnamedReviewer,
}

/// Whether a reviewer may review a synthesis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerEligibility {
    /// The reviewer is a participant, is not the chair and is not the synthesis author.
    Eligible {
        reviewer_agent_id: String,
        reviewer_role: Option<RoleKind>,
    },
    /// The reviewer may not review this synthesis.
    Refused(ReviewerRefusal),
}

impl ReviewerEligibility {
    /// Whether the review may proceed.
    pub fn is_eligible(&self) -> bool {
        matches!(self, ReviewerEligibility::Eligible { .. })
    }

    /// The refusal, when the review was refused.
    pub fn refusal(&self) -> Option<&ReviewerRefusal> {
        match self {
            ReviewerEligibility::Eligible { .. } => None,
            ReviewerEligibility::Refused(refusal) => Some(refusal),
        }
    }
}

/// Check that `reviewer_agent_id` may review `synthesis`.
///
/// `participants` is the round's participant roster, `chair_agent_id` is the round's chair, and
/// `reviewer_role` is the duty the reviewer holds when the caller knows it. The three rules are ordered so
/// the most specific refusal is reported first: the chair is refused as chair even when it also authored the
/// synthesis, because "the chair cannot self-certify" is the rule the contract states.
pub fn check_reviewer_eligibility(
    synthesis: &SynthesisPosition,
    reviewer_agent_id: &str,
    participants: &[&str],
    chair_agent_id: &str,
    reviewer_role: Option<RoleKind>,
) -> ReviewerEligibility {
    if reviewer_agent_id.trim().is_empty() {
        return ReviewerEligibility::Refused(ReviewerRefusal::UnnamedReviewer);
    }
    if reviewer_agent_id == chair_agent_id {
        return ReviewerEligibility::Refused(ReviewerRefusal::Chair);
    }
    if reviewer_agent_id == synthesis.author_agent_id {
        return ReviewerEligibility::Refused(ReviewerRefusal::SynthesisAuthor {
            author_agent_id: synthesis.author_agent_id.clone(),
        });
    }
    if !participants.contains(&reviewer_agent_id) {
        return ReviewerEligibility::Refused(ReviewerRefusal::NotAParticipant);
    }

    ReviewerEligibility::Eligible {
        reviewer_agent_id: reviewer_agent_id.to_string(),
        reviewer_role,
    }
}

/// The `CRITIQUE` target a synthesis review addresses.
///
/// The review reuses the existing `CRITIQUE` message type pointed at the synthesis position id. This helper
/// exists so the target is derived from the synthesis rather than spelled at each call site, and so it is
/// visible in one place that no new message type is involved.
pub fn critique_target(synthesis: &SynthesisPosition) -> &str {
    synthesis.position_id.as_str()
}

/// The lineage group of a synthesis's author, when the caller knows the adapter type.
///
/// Reported alongside a review so a reader can see whether the review crossed a lineage boundary. The check
/// does not require it: DEC-052 requires a non-chair reviewer, and a different lineage is a quality fact
/// rather than a prerequisite.
pub fn author_lineage(agent_type: &str) -> Option<LineageGroup> {
    crate::lineage::adapter_lineage(agent_type).map(|source| source.group)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthesis() -> SynthesisPosition {
        SynthesisPosition::new("pos_syn", "agent-hermes", &["pos_a", "pos_b"])
    }

    #[test]
    fn a_complete_synthesis_passes_and_reports_its_coverage() {
        let coverage = check_synthesis_coverage(&synthesis(), &["pos_a", "pos_b"]);
        assert_eq!(
            coverage,
            Ok(SynthesisCoverage {
                complete: true,
                omitted_position_ids: Vec::new(),
                covered_count: 2,
                surviving_count: 2,
            })
        );
    }

    #[test]
    fn an_omitted_position_is_detected_and_named() {
        let partial = SynthesisPosition::new("pos_syn", "agent-hermes", &["pos_a"]);
        let gap = check_synthesis_coverage(&partial, &["pos_a", "pos_b", "pos_c"]);
        assert_eq!(
            gap,
            Err(CoverageGap::OmittedPositions {
                position_ids: vec!["pos_b".to_string(), "pos_c".to_string()],
            })
        );

        // The omission list is stable regardless of the survivor order it was computed from.
        let reordered = check_synthesis_coverage(&partial, &["pos_c", "pos_b", "pos_a"]);
        assert_eq!(
            reordered,
            Err(CoverageGap::OmittedPositions {
                position_ids: vec!["pos_b".to_string(), "pos_c".to_string()],
            })
        );
    }

    #[test]
    fn a_synthesis_that_cites_nothing_is_refused_explicitly() {
        let empty = SynthesisPosition::new("pos_syn", "agent-hermes", &[]);
        assert_eq!(
            check_synthesis_coverage(&empty, &["pos_a"]),
            Err(CoverageGap::NoCitations)
        );
    }

    #[test]
    fn a_self_referential_synthesis_is_refused() {
        let circular = SynthesisPosition::new("pos_syn", "agent-hermes", &["pos_syn", "pos_a"]);
        assert_eq!(
            check_synthesis_coverage(&circular, &["pos_a"]),
            Err(CoverageGap::SelfReferential)
        );
    }

    #[test]
    fn citing_a_withdrawn_position_is_not_an_omission() {
        let generous =
            SynthesisPosition::new("pos_syn", "agent-hermes", &["pos_a", "pos_withdrawn"]);
        let coverage =
            check_synthesis_coverage(&generous, &["pos_a"]).unwrap_or(SynthesisCoverage {
                complete: false,
                omitted_position_ids: Vec::new(),
                covered_count: 0,
                surviving_count: 0,
            });
        assert!(coverage.complete);
        assert!(!coverage.has_omissions());
        assert_eq!(coverage.covered_count, 1);
        assert_eq!(coverage.surviving_count, 1);
        assert_eq!(extra_citations(&generous, &coverage), 1);
    }

    #[test]
    fn the_chair_can_never_review_its_own_synthesis() {
        let eligibility = check_reviewer_eligibility(
            &synthesis(),
            "agent-hermes",
            &["agent-hermes", "agent-kilo"],
            "agent-hermes",
            None,
        );
        assert_eq!(
            eligibility,
            ReviewerEligibility::Refused(ReviewerRefusal::Chair)
        );
        assert!(!eligibility.is_eligible());
        assert_eq!(eligibility.refusal(), Some(&ReviewerRefusal::Chair));
    }

    #[test]
    fn the_synthesis_author_cannot_review_its_own_synthesis_even_when_not_chair() {
        let eligibility = check_reviewer_eligibility(
            &synthesis(),
            "agent-hermes",
            &["agent-hermes", "agent-kilo"],
            "agent-kilo",
            None,
        );
        assert_eq!(
            eligibility,
            ReviewerEligibility::Refused(ReviewerRefusal::SynthesisAuthor {
                author_agent_id: "agent-hermes".to_string(),
            })
        );
    }

    #[test]
    fn a_non_chair_participant_reviews_the_synthesis() {
        let eligibility = check_reviewer_eligibility(
            &synthesis(),
            "agent-opencode",
            &["agent-hermes", "agent-opencode"],
            "agent-hermes",
            Some(RoleKind::Verifier),
        );
        assert_eq!(
            eligibility,
            ReviewerEligibility::Eligible {
                reviewer_agent_id: "agent-opencode".to_string(),
                reviewer_role: Some(RoleKind::Verifier),
            }
        );
        assert!(eligibility.is_eligible());
        assert!(eligibility.refusal().is_none());
    }

    #[test]
    fn a_non_participant_or_unnamed_reviewer_is_refused() {
        let outsider = check_reviewer_eligibility(
            &synthesis(),
            "agent-stranger",
            &["agent-hermes"],
            "agent-hermes",
            None,
        );
        assert_eq!(
            outsider,
            ReviewerEligibility::Refused(ReviewerRefusal::NotAParticipant)
        );

        let unnamed =
            check_reviewer_eligibility(&synthesis(), "  ", &["agent-hermes"], "agent-hermes", None);
        assert_eq!(
            unnamed,
            ReviewerEligibility::Refused(ReviewerRefusal::UnnamedReviewer)
        );
    }

    #[test]
    fn the_review_targets_the_synthesis_position_by_the_existing_critique_message() {
        // No new message type exists in this crate; the target is the synthesis position id, which is what an
        // existing CRITIQUE addresses.
        assert_eq!(critique_target(&synthesis()), "pos_syn");
        assert_eq!(synthesis().position_id, critique_target(&synthesis()));
    }

    #[test]
    fn lineage_of_the_author_is_reported_when_the_adapter_is_known() {
        assert_eq!(
            author_lineage("HERMES_AGENT"),
            Some(LineageGroup::HermesAgent)
        );
        assert_eq!(
            author_lineage("KILO_CODE"),
            Some(LineageGroup::OpencodeFork)
        );
        assert_eq!(author_lineage("SOMETHING_ELSE"), None);
    }
}
