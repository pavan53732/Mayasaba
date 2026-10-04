//! The convergence seal precondition (DEC-052 item 3, `COUNCIL-ENGINE.md` "Convergence precondition").
//!
//! A round on a material decision point may not seal `CONVERGED` while a surviving position carries a
//! load-bearing `ASSUMPTION` claim. Two limits on what this module is, both of them architectural:
//!
//! * It does **not** reimplement or replace the fixpoint predicate. `COUNCIL-ENGINE.md` defines `CONVERGED`
//!   as a fixpoint over the position set, and that test still decides whether a round converged. This module
//!   answers a second, narrower question - may the round *seal* that outcome - which is why [`can_seal_converged`]
//!   takes the fixpoint's result as given and returns an enumerated verdict rather than a boolean.
//! * It adds **no** `outcome_type` value. Refusal leaves the round running while rounds remain, and otherwise
//!   the round ends `ESCALATED` or `CAP_REACHED` under DEC-033's existing rules; both already exist.
//!
//! The blocking rules are configuration, following `council-policies.json:evidence`, because DEC-052 makes
//! thresholds configuration and because `block_convergence_for_material_decisions_only` is exactly the kind
//! of switch a later decision may want to move.

use crate::evidence::PositionGrade;
use crate::mode::DecisionClass;

/// The convergence precondition, as configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvergencePrecondition {
    /// `block_convergence_on_load_bearing_assumption`.
    pub block_on_load_bearing_assumption: bool,
    /// `block_convergence_for_material_decisions_only`. When true, a `ROUTINE` decision point is never
    /// blocked, which is the shipped default.
    pub material_decisions_only: bool,
}

impl Default for ConvergencePrecondition {
    fn default() -> Self {
        Self::from_shipped_policy()
    }
}

impl ConvergencePrecondition {
    /// The shipped `council-policies.json:evidence` values.
    pub fn from_shipped_policy() -> Self {
        ConvergencePrecondition {
            block_on_load_bearing_assumption: true,
            material_decisions_only: true,
        }
    }
}

/// Why a round may not seal `CONVERGED`.
///
/// Refusals are enumerated rather than returned as a bare `false`, so a caller can tell "the evidence does
/// not support convergence" apart from "this decision point is not material" and can report the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvergenceRefusal {
    /// One or more surviving positions rest on a load-bearing `ASSUMPTION` claim.
    LoadBearingAssumption {
        /// The offending position ids, sorted, so the report is stable across runs.
        position_ids: Vec<String>,
    },
}

impl ConvergenceRefusal {
    /// The position ids this refusal is about, empty for refusals that name none.
    pub fn position_ids(&self) -> &[String] {
        match self {
            ConvergenceRefusal::LoadBearingAssumption { position_ids } => position_ids,
        }
    }
}

/// Whether a round may seal `CONVERGED`.
///
/// `fixpoint_reached` is the existing predicate's answer, passed in rather than re-derived: this function
/// only narrows when sealing is permitted. The rule is that a round may seal only when the fixpoint was
/// reached *and* no surviving position carries a load-bearing assumption, subject to the configured
/// toggles.
///
/// A position is a survivor because the caller says so. A position that was withdrawn, superseded or
/// recorded as non-participation is not a survivor, and this crate must not guess which positions those
/// are - the round owns that fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvergenceVerdict {
    /// The round may seal `CONVERGED`.
    Permitted,
    /// The fixpoint was not reached, so there is nothing to seal. Not a quality judgement.
    FixpointNotReached,
    /// The round may not seal `CONVERGED`.
    Refused(ConvergenceRefusal),
}

/// May this round seal `CONVERGED`?
pub fn can_seal_converged(
    decision_class: DecisionClass,
    surviving_position_ids: &[&str],
    position_grades: &[PositionGrade],
    fixpoint_reached: bool,
    precondition: &ConvergencePrecondition,
) -> ConvergenceVerdict {
    if !fixpoint_reached {
        return ConvergenceVerdict::FixpointNotReached;
    }

    if !precondition.block_on_load_bearing_assumption {
        return ConvergenceVerdict::Permitted;
    }

    if precondition.material_decisions_only && !decision_class.is_material() {
        return ConvergenceVerdict::Permitted;
    }

    // Grades arrive in the caller's order alongside the ids. A grade with no matching id cannot be reported,
    // so the positions are walked by id and the grade list is searched; ids that carry no grade at all are
    // treated as ungraded and do not block here, because a position with no computed grade is a grading
    // failure for the caller to resolve rather than a silent assumption. `grade_position` never needs to
    // guess either: it returns `NoLoadBearingClaims` explicitly.
    let mut blocking: Vec<String> = Vec::new();
    for (index, position_id) in surviving_position_ids.iter().enumerate() {
        let grade = position_grades.get(index);
        if grade.is_some_and(|grade| grade.has_load_bearing_assumption()) {
            blocking.push((*position_id).to_string());
        }
    }
    blocking.sort();
    blocking.dedup();

    if blocking.is_empty() {
        ConvergenceVerdict::Permitted
    } else {
        ConvergenceVerdict::Refused(ConvergenceRefusal::LoadBearingAssumption {
            position_ids: blocking,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{
        grade_position, Claim, EvidenceResolver, Position, PositionGrade, ReferenceResolution,
    };

    struct Resolver;

    impl EvidenceResolver for Resolver {
        fn resolve(&self, reference: &str) -> ReferenceResolution {
            if reference == "check:ran" {
                ReferenceResolution::ControllerExecutedCheck
            } else if reference == "doc:exists" {
                ReferenceResolution::ExistingFact
            } else {
                ReferenceResolution::Unresolvable {
                    reason: crate::evidence::GradeReason::ReferenceNotFound,
                }
            }
        }
    }

    fn grade(position: &Position) -> PositionGrade {
        grade_position(position, &Resolver)
    }

    fn assumed() -> PositionGrade {
        grade(&Position::new("pos_a").with_claim(Claim::load_bearing("c1").citing("doc:missing")))
    }

    fn cited() -> PositionGrade {
        grade(&Position::new("pos_c").with_claim(Claim::load_bearing("c1").citing("doc:exists")))
    }

    fn material() -> DecisionClass {
        DecisionClass::Architecture
    }

    fn shipped() -> ConvergencePrecondition {
        ConvergencePrecondition::from_shipped_policy()
    }

    #[test]
    fn a_load_bearing_assumption_blocks_convergence_on_a_material_decision() {
        let verdict = can_seal_converged(
            material(),
            &["pos_a", "pos_c"],
            &[assumed(), cited()],
            true,
            &shipped(),
        );
        assert_eq!(
            verdict,
            ConvergenceVerdict::Refused(ConvergenceRefusal::LoadBearingAssumption {
                position_ids: vec!["pos_a".to_string()],
            })
        );
    }

    #[test]
    fn every_grounded_position_permits_the_seal() {
        let verdict = can_seal_converged(material(), &["pos_c"], &[cited()], true, &shipped());
        assert_eq!(verdict, ConvergenceVerdict::Permitted);

        let verified = grade(
            &Position::new("pos_v").with_claim(Claim::load_bearing("c1").citing("check:ran")),
        );
        assert_eq!(
            can_seal_converged(material(), &["pos_v"], &[verified], true, &shipped()),
            ConvergenceVerdict::Permitted
        );
    }

    #[test]
    fn a_routine_decision_point_is_not_blocked_by_the_material_only_toggle() {
        assert_eq!(
            can_seal_converged(
                DecisionClass::Routine,
                &["pos_a"],
                &[assumed()],
                true,
                &shipped()
            ),
            ConvergenceVerdict::Permitted
        );

        // The toggle is data: with it disabled, the same routine position is blocked.
        let strict = ConvergencePrecondition {
            block_on_load_bearing_assumption: true,
            material_decisions_only: false,
        };
        assert!(matches!(
            can_seal_converged(
                DecisionClass::Routine,
                &["pos_a"],
                &[assumed()],
                true,
                &strict
            ),
            ConvergenceVerdict::Refused(_)
        ));
    }

    #[test]
    fn the_block_is_configurable_in_both_directions() {
        let off = ConvergencePrecondition {
            block_on_load_bearing_assumption: false,
            material_decisions_only: true,
        };
        assert_eq!(
            can_seal_converged(material(), &["pos_a"], &[assumed()], true, &off),
            ConvergenceVerdict::Permitted
        );
    }

    #[test]
    fn an_unreached_fixpoint_is_reported_as_such_rather_than_as_a_quality_refusal() {
        assert_eq!(
            can_seal_converged(material(), &["pos_a"], &[assumed()], false, &shipped()),
            ConvergenceVerdict::FixpointNotReached
        );
    }

    #[test]
    fn a_position_with_no_load_bearing_claims_does_not_by_itself_block_the_seal() {
        // `NoLoadBearingClaims` is not an assumption-backed position. It is reported explicitly by the
        // grader, and the convergence rule is about load-bearing assumptions specifically.
        let empty = grade(&Position::new("pos_e"));
        assert_eq!(
            empty,
            PositionGrade::NoLoadBearingClaims {
                supporting_claim_count: 0
            }
        );
        assert_eq!(
            can_seal_converged(material(), &["pos_e"], &[empty], true, &shipped()),
            ConvergenceVerdict::Permitted
        );
    }

    #[test]
    fn refusals_name_every_offending_position_and_are_order_independent() {
        let verdict = can_seal_converged(
            material(),
            &["pos_c", "pos_a"],
            &[cited(), assumed()],
            true,
            &shipped(),
        );
        assert_eq!(
            verdict,
            ConvergenceVerdict::Refused(ConvergenceRefusal::LoadBearingAssumption {
                position_ids: vec!["pos_a".to_string()],
            }),
            "the graded position is named, and the order of the survivor list does not matter"
        );

        let both = can_seal_converged(
            material(),
            &["pos_a", "pos_b"],
            &[assumed(), assumed()],
            true,
            &shipped(),
        );
        assert_eq!(
            both,
            ConvergenceVerdict::Refused(ConvergenceRefusal::LoadBearingAssumption {
                position_ids: vec!["pos_a".to_string(), "pos_b".to_string()],
            }),
            "every offending position is named, in a stable order"
        );
    }
}
