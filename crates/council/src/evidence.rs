//! Controller-computed evidence grades (DEC-052 item 2, `COUNCIL-ENGINE.md` "Evidence grades").
//!
//! The grade of a claim is `ASSUMPTION < CITED < VERIFIED`, computed here from how the claim's references
//! resolve. There is deliberately **no grade parameter anywhere in this module**: an agent-supplied grade
//! is not merely ignored, it is unrepresentable, which is a stronger property than validating one and is
//! the reason the rules below are structural rather than defensive.
//!
//! Reference resolution is behind [`EvidenceResolver`] so grading is testable without a database and so
//! this crate never learns what a repository fact or an `evidence` row is stored as. An implementation
//! backed by `crates/storage` resolves against the existing `evidence` table; this crate only asks whether
//! a reference resolved and whether what it resolved to was a controller-executed check.

use std::collections::BTreeSet;

/// How a reference argument resolved.
///
/// The failure case is a single variant with a reason rather than several, because every way of failing to
/// resolve grades the same: `CITED` requires *every* reference to resolve, so one unresolvable reference is
/// enough, and a caller that needs to distinguish "no such row" from "points at something that is not a
/// check" reads [`GradeReason`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceResolution {
    /// The reference resolved to nothing this controller can vouch for.
    Unresolvable {
        /// Why it did not resolve, for the record's basis.
        reason: GradeReason,
    },
    /// The reference resolved to an existing repository fact, document or `evidence` row.
    ExistingFact,
    /// The reference resolved to a controller-executed check or spike stored in `evidence`.
    ControllerExecutedCheck,
}

impl ReferenceResolution {
    /// The controller-authoritative reference: a check or spike the controller itself executed.
    pub fn controller_executed_check() -> Self {
        ReferenceResolution::ControllerExecutedCheck
    }

    /// An existing repository fact, document or `evidence` row.
    pub fn existing_fact() -> Self {
        ReferenceResolution::ExistingFact
    }

    /// A reference that resolved to nothing.
    pub fn unresolvable(reason: GradeReason) -> Self {
        ReferenceResolution::Unresolvable { reason }
    }

    /// Whether the reference resolved at all.
    ///
    /// `CITED` turns on this question alone, so it has one home rather than being re-derived per call site.
    pub fn resolved(&self) -> bool {
        !matches!(self, ReferenceResolution::Unresolvable { .. })
    }

    /// Whether the reference is backed by controller execution.
    pub fn is_controller_executed_check(&self) -> bool {
        matches!(self, ReferenceResolution::ControllerExecutedCheck)
    }
}

/// Why a claim graded the way it did. Recorded as the claim's basis so a grade is explainable without
/// re-running the grader against a database that may since have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradeReason {
    /// The claim cited no reference at all.
    NoReferences,
    /// A cited reference does not resolve to anything this controller can vouch for.
    ReferenceNotFound,
    /// A cited reference resolved to something that is not a controller-executed check or spike.
    ReferenceIsNotControllerExecutedCheck,
    /// Every load-bearing claim graded at or above this level.
    WeakestLoadBearingClaim,
    /// The position carries no load-bearing claim, so it has no grade from which to reason.
    NoLoadBearingClaims,
}

impl GradeReason {
    /// The stable token stored in a claim grade's `basis_json`.
    pub fn as_str(self) -> &'static str {
        match self {
            GradeReason::NoReferences => "NO_REFERENCES",
            GradeReason::ReferenceNotFound => "REFERENCE_NOT_FOUND",
            GradeReason::ReferenceIsNotControllerExecutedCheck => {
                "REFERENCE_IS_NOT_CONTROLLER_EXECUTED_CHECK"
            }
            GradeReason::WeakestLoadBearingClaim => "WEAKEST_LOAD_BEARING_CLAIM",
            GradeReason::NoLoadBearingClaims => "NO_LOAD_BEARING_CLAIMS",
        }
    }
}

impl std::fmt::Display for GradeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolves a claim's reference argument.
///
/// Implementations must be pure lookups. A resolver that graded, inferred or guessed would move the grade
/// out of the controller, which is the thing this trait exists to prevent.
pub trait EvidenceResolver {
    /// Resolve one reference. The argument's syntax is the caller's; this crate treats it opaquely.
    fn resolve(&self, reference: &str) -> ReferenceResolution;
}

/// The controller-computed grade of a claim.
///
/// Ordered `Assumption < Cited < Verified`, matching the contract's comparison. A position's grade is the
/// minimum over its load-bearing claims, so an ordering that disagreed with the vocabulary would silently
/// invert the weakest-link rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClaimGrade {
    /// Nothing resolvable backs the claim.
    Assumption,
    /// Every reference resolves to an existing repository fact, document or `evidence` row.
    Cited,
    /// A controller-executed check or spike stored in `evidence` backs the claim.
    Verified,
}

impl ClaimGrade {
    /// Every grade, weakest first.
    pub const ALL: [ClaimGrade; 3] = [
        ClaimGrade::Assumption,
        ClaimGrade::Cited,
        ClaimGrade::Verified,
    ];

    /// The contract spelling, as stored in `council_claim_grades.grade`.
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimGrade::Assumption => "ASSUMPTION",
            ClaimGrade::Cited => "CITED",
            ClaimGrade::Verified => "VERIFIED",
        }
    }

    /// Parse the contract spelling.
    pub fn parse(value: &str) -> Option<Self> {
        ClaimGrade::ALL
            .into_iter()
            .find(|grade| grade.as_str() == value)
    }

    /// The weaker of two grades. This is the weakest-link combinator.
    pub fn weakest(self, other: ClaimGrade) -> ClaimGrade {
        if other < self {
            other
        } else {
            self
        }
    }
}

impl std::fmt::Display for ClaimGrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One claim inside a position.
///
/// There is no `grade` field, and that is the point: the struct that describes a claim to the grader has
/// nowhere to hold a claimed grade, so an agent-authored payload cannot carry one into this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// Stable identity of the claim, so a computed grade can be linked back to what it grades.
    pub claim_id: String,
    /// Evidence references cited in support.
    pub references: Vec<String>,
    /// Whether the claim is peripheral.
    ///
    /// A claim is load-bearing unless it is flagged supporting, so the default of the constructor below is
    /// `false` for this field and callers opt *out* of load-bearing rather than into it.
    pub supporting: bool,
}

impl Claim {
    /// A load-bearing claim. The default, because DEC-052 makes load-bearing the unmarked case.
    pub fn load_bearing(claim_id: &str) -> Self {
        Claim {
            claim_id: claim_id.to_string(),
            references: Vec::new(),
            supporting: false,
        }
    }

    /// A claim explicitly flagged supporting, which is therefore excluded from its position's grade.
    pub fn supporting(claim_id: &str) -> Self {
        Claim {
            claim_id: claim_id.to_string(),
            references: Vec::new(),
            supporting: true,
        }
    }

    /// Cite a reference.
    pub fn citing(mut self, reference: &str) -> Self {
        self.references.push(reference.to_string());
        self
    }

    /// Whether this claim contributes to its position's grade.
    pub fn is_load_bearing(&self) -> bool {
        !self.supporting
    }
}

/// A position, as its claims. A position has no grade field either, for the same reason a claim has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub position_id: String,
    pub claims: Vec<Claim>,
}

impl Position {
    /// A position with no claims yet.
    pub fn new(position_id: &str) -> Self {
        Position {
            position_id: position_id.to_string(),
            claims: Vec::new(),
        }
    }

    /// Add a claim.
    pub fn with_claim(mut self, claim: Claim) -> Self {
        self.claims.push(claim);
        self
    }

    /// The position's load-bearing claims.
    pub fn load_bearing_claims(&self) -> impl Iterator<Item = &Claim> {
        self.claims.iter().filter(|claim| claim.is_load_bearing())
    }
}

/// A graded claim, with the basis the grade rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimGradeBasis {
    pub claim_id: String,
    pub grade: ClaimGrade,
    pub reason: GradeReason,
    /// References that resolved, sorted and de-duplicated.
    pub resolved_references: Vec<String>,
    /// References that did not resolve, sorted and de-duplicated.
    pub unresolved_references: Vec<String>,
}

impl ClaimGradeBasis {
    /// The grade.
    pub fn grade(&self) -> ClaimGrade {
        self.grade
    }
}

/// Grade one claim from how its references resolve.
///
/// `ASSUMPTION` when the claim cites nothing or any citation fails to resolve, `CITED` when every citation
/// resolves to an existing fact, `VERIFIED` only when at least one citation is a controller-executed check
/// or spike and none is unresolvable. A single unresolvable reference defeats the whole claim, because
/// `CITED` is defined as *every* reference resolving.
pub fn grade_claim(claim: &Claim, resolver: &dyn EvidenceResolver) -> ClaimGradeBasis {
    let mut resolved = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    let mut has_controller_check = false;

    for reference in &claim.references {
        match resolver.resolve(reference) {
            ReferenceResolution::ExistingFact => {
                resolved.insert(reference.clone());
            }
            ReferenceResolution::ControllerExecutedCheck => {
                has_controller_check = true;
                resolved.insert(reference.clone());
            }
            ReferenceResolution::Unresolvable { .. } => {
                unresolved.insert(reference.clone());
            }
        }
    }

    let (grade, reason) = if !unresolved.is_empty() {
        (ClaimGrade::Assumption, GradeReason::ReferenceNotFound)
    } else if has_controller_check {
        (ClaimGrade::Verified, GradeReason::WeakestLoadBearingClaim)
    } else if !resolved.is_empty() {
        (ClaimGrade::Cited, GradeReason::WeakestLoadBearingClaim)
    } else {
        (ClaimGrade::Assumption, GradeReason::NoReferences)
    };

    ClaimGradeBasis {
        claim_id: claim.claim_id.clone(),
        grade,
        reason,
        resolved_references: resolved.into_iter().collect(),
        unresolved_references: unresolved.into_iter().collect(),
    }
}

/// A position's grade, or the explicit statement that it has none.
///
/// The `NoLoadBearingClaims` variant is the whole reason this is an enum. A position with no load-bearing
/// claims has no grade, and returning `ASSUMPTION` for it would be a silent default that reads like a
/// computed fact: it would falsely report opinion, and it would make an all-supporting position
/// indistinguishable from one resting on an unresolvable assumption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionGrade {
    /// The weakest grade among the position's load-bearing claims.
    WeakestClaim {
        grade: ClaimGrade,
        /// Every load-bearing claim's grade, in the position's own claim order.
        claims: Vec<ClaimGradeBasis>,
    },
    /// The position carries no load-bearing claim, so it has no grade.
    ///
    /// DEC-052 says a position's grade is its weakest load-bearing claim; with no load-bearing claim there
    /// is nothing to weaken, and treating the position as `ASSUMPTION` would conflate "nothing asserted" with
    /// "asserted without support".
    NoLoadBearingClaims { supporting_claim_count: usize },
}

impl PositionGrade {
    /// The grade, when the position has one.
    pub fn grade(&self) -> Option<ClaimGrade> {
        match self {
            PositionGrade::WeakestClaim { grade, .. } => Some(*grade),
            PositionGrade::NoLoadBearingClaims { .. } => None,
        }
    }

    /// Whether the position carries a load-bearing `ASSUMPTION` claim.
    ///
    /// This is the exact question the convergence precondition asks, and it is answered here so the
    /// precondition does not re-derive it from the claim list and drift from the grading rule.
    pub fn has_load_bearing_assumption(&self) -> bool {
        matches!(
            self,
            PositionGrade::WeakestClaim {
                grade: ClaimGrade::Assumption,
                ..
            }
        )
    }

    /// Why this grade was reached.
    pub fn reason(&self) -> GradeReason {
        match self {
            PositionGrade::WeakestClaim { .. } => GradeReason::WeakestLoadBearingClaim,
            PositionGrade::NoLoadBearingClaims { .. } => GradeReason::NoLoadBearingClaims,
        }
    }

    /// The graded load-bearing claims, empty when there are none.
    pub fn claim_grades(&self) -> &[ClaimGradeBasis] {
        match self {
            PositionGrade::WeakestClaim { claims, .. } => claims,
            PositionGrade::NoLoadBearingClaims { .. } => &[],
        }
    }
}

/// Grade a position from its load-bearing claims.
pub fn grade_position(position: &Position, resolver: &dyn EvidenceResolver) -> PositionGrade {
    let mut graded: Vec<ClaimGradeBasis> = Vec::new();
    let mut weakest: Option<ClaimGrade> = None;

    for claim in position.load_bearing_claims() {
        let basis = grade_claim(claim, resolver);
        weakest = Some(match weakest {
            Some(current) => current.weakest(basis.grade),
            None => basis.grade,
        });
        graded.push(basis);
    }

    match weakest {
        Some(grade) => PositionGrade::WeakestClaim {
            grade,
            claims: graded,
        },
        None => PositionGrade::NoLoadBearingClaims {
            supporting_claim_count: position.claims.len(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resolver over a fixed set of facts and controller-executed checks, so grading is tested without a
    /// database. Anything absent from both sets is unresolvable, which is the case the tests care about.
    struct MapResolver {
        facts: Vec<&'static str>,
        checks: Vec<&'static str>,
    }

    impl EvidenceResolver for MapResolver {
        fn resolve(&self, reference: &str) -> ReferenceResolution {
            if self.checks.contains(&reference) {
                ReferenceResolution::ControllerExecutedCheck
            } else if self.facts.contains(&reference) {
                ReferenceResolution::ExistingFact
            } else {
                ReferenceResolution::Unresolvable {
                    reason: GradeReason::ReferenceNotFound,
                }
            }
        }
    }

    fn resolver() -> MapResolver {
        MapResolver {
            facts: vec!["doc:ARCHITECTURE.md#2", "evidence:ev_1"],
            checks: vec!["check:cargo-test"],
        }
    }

    #[test]
    fn a_claim_with_no_resolvable_reference_grades_assumption() {
        let bare = Claim::load_bearing("c1");
        assert_eq!(
            grade_claim(&bare, &resolver()).grade,
            ClaimGrade::Assumption
        );
        assert_eq!(
            grade_claim(&bare, &resolver()).reason,
            GradeReason::NoReferences
        );

        let dangling = bare.clone().citing("doc:does-not-exist.md");
        let basis = grade_claim(&dangling, &resolver());
        assert_eq!(basis.grade, ClaimGrade::Assumption);
        assert_eq!(basis.reason, GradeReason::ReferenceNotFound);
        assert_eq!(
            basis.unresolved_references,
            vec!["doc:does-not-exist.md".to_string()]
        );
    }

    #[test]
    fn every_reference_resolving_grades_cited() {
        let cited = Claim::load_bearing("c1")
            .citing("doc:ARCHITECTURE.md#2")
            .citing("evidence:ev_1");
        let basis = grade_claim(&cited, &resolver());
        assert_eq!(basis.grade, ClaimGrade::Cited);
        assert_eq!(basis.resolved_references.len(), 2);
        assert!(basis.unresolved_references.is_empty());
    }

    #[test]
    fn one_unresolvable_reference_defeats_the_whole_claim() {
        let mixed = Claim::load_bearing("c1")
            .citing("doc:ARCHITECTURE.md#2")
            .citing("evidence:ev_1")
            .citing("evidence:missing");
        let basis = grade_claim(&mixed, &resolver());
        assert_eq!(
            basis.grade,
            ClaimGrade::Assumption,
            "CITED requires every reference to resolve"
        );
        assert_eq!(
            basis.unresolved_references,
            vec!["evidence:missing".to_string()]
        );
    }

    #[test]
    fn a_controller_executed_check_grades_verified() {
        let checked = Claim::load_bearing("c1").citing("check:cargo-test");
        assert_eq!(
            grade_claim(&checked, &resolver()).grade,
            ClaimGrade::Verified
        );

        // A check that cannot be resolved is still not evidence.
        let claimed = Claim::load_bearing("c2").citing("check:never-ran");
        assert_eq!(
            grade_claim(&claimed, &resolver()).grade,
            ClaimGrade::Assumption
        );
    }

    #[test]
    fn a_positions_grade_is_its_weakest_load_bearing_claim() {
        let position = Position::new("pos_1")
            .with_claim(Claim::load_bearing("c1").citing("check:cargo-test"))
            .with_claim(Claim::load_bearing("c2").citing("doc:ARCHITECTURE.md#2"))
            .with_claim(Claim::load_bearing("c3").citing("nothing-resolves"));
        let grade = grade_position(&position, &resolver());
        assert_eq!(grade.grade(), Some(ClaimGrade::Assumption));
        assert!(grade.has_load_bearing_assumption());
        assert_eq!(
            grade.claim_grades().len(),
            3,
            "every load-bearing claim is reported"
        );
    }

    #[test]
    fn supporting_claims_do_not_lower_a_positions_grade() {
        let position = Position::new("pos_1")
            .with_claim(Claim::load_bearing("c1").citing("check:cargo-test"))
            .with_claim(Claim::supporting("c2").citing("nothing-resolves"));
        let grade = grade_position(&position, &resolver());
        assert_eq!(grade.grade(), Some(ClaimGrade::Verified));
        assert!(!grade.has_load_bearing_assumption());
        assert_eq!(
            grade.claim_grades().len(),
            1,
            "the supporting claim is excluded"
        );
    }

    #[test]
    fn a_position_with_no_load_bearing_claims_is_reported_explicitly() {
        let empty = Position::new("pos_1");
        let grade = grade_position(&empty, &resolver());
        assert_eq!(
            grade,
            PositionGrade::NoLoadBearingClaims {
                supporting_claim_count: 0
            }
        );
        assert_eq!(grade.grade(), None, "no silent default to ASSUMPTION");
        assert_eq!(grade.reason(), GradeReason::NoLoadBearingClaims);

        let only_supporting = Position::new("pos_2")
            .with_claim(Claim::supporting("c1").citing("doc:ARCHITECTURE.md#2"));
        assert_eq!(
            grade_position(&only_supporting, &resolver()),
            PositionGrade::NoLoadBearingClaims {
                supporting_claim_count: 1
            }
        );
    }

    #[test]
    fn the_grade_vocabulary_is_ordered_weakest_first() {
        assert!(ClaimGrade::Assumption < ClaimGrade::Cited);
        assert!(ClaimGrade::Cited < ClaimGrade::Verified);
        assert_eq!(
            ClaimGrade::Verified.weakest(ClaimGrade::Assumption),
            ClaimGrade::Assumption
        );
        assert_eq!(
            ClaimGrade::Cited.weakest(ClaimGrade::Cited),
            ClaimGrade::Cited
        );
        for grade in ClaimGrade::ALL {
            assert_eq!(ClaimGrade::parse(grade.as_str()), Some(grade));
        }
        assert_eq!(ClaimGrade::parse("CERTIFIED"), None);
    }
}
