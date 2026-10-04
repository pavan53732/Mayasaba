//! Council mode selection (DEC-052 item 1, `COUNCIL-ENGINE.md` "Council modes").
//!
//! A material decision point receives exactly one mode, computed here from structured inputs. No LLM
//! chooses it, and no agent may select or downgrade one - which is why [`select_mode`] takes no agent, no
//! session and no transcript, and why an agent-authored mode has nowhere to be expressed in this API.
//!
//! Three rules shape the code:
//!
//! * **A user override always wins, in both directions.** DEC-013 makes the user the authority, and
//!   `council-policies.json` states `high_risk_classes_default_mode` is a default and not a floor. There is
//!   deliberately no code here that refuses a downward override; refusing one would be this crate deciding
//!   it outranks the user. The override is recorded and reported instead, which is what makes it explicit
//!   rather than silent.
//! * **Thresholds are data.** `ModeThresholds` is passed in, never read from a constant, because DEC-052
//!   makes these configuration that `validate_configuration`/`update_configuration` can change.
//! * **A non-material decision point is one whose class is `ROUTINE`.** Only a material point is persisted
//!   as a controller record of a decision, and `SOLO` opens no round.

use std::collections::BTreeSet;

use crate::clock::Clock;

/// The selector version recorded with every selection.
///
/// It identifies the algorithm, not the thresholds: thresholds are carried in the record's inputs, so a
/// configuration change does not have to mint a new selector version.
pub const DEFAULT_SELECTOR_VERSION: &str = "mode-selector-v1";

/// The `decisions.class` vocabulary DEC-052 reuses rather than shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecisionClass {
    Architecture,
    StackTechnology,
    Irreversible,
    Security,
    DataLoss,
    Routine,
}

impl DecisionClass {
    /// Every class, in the order the contract lists them.
    ///
    /// A caller iterating the vocabulary (to build a configuration form, or to check that all six survive a
    /// round trip) uses this rather than restating it.
    pub const ALL: [DecisionClass; 6] = [
        DecisionClass::Architecture,
        DecisionClass::StackTechnology,
        DecisionClass::Irreversible,
        DecisionClass::Security,
        DecisionClass::DataLoss,
        DecisionClass::Routine,
    ];

    /// The contract spelling, as stored in `decisions.class` and in `council_mode_selections.decision_class`.
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionClass::Architecture => "ARCHITECTURE",
            DecisionClass::StackTechnology => "STACK_TECHNOLOGY",
            DecisionClass::Irreversible => "IRREVERSIBLE",
            DecisionClass::Security => "SECURITY",
            DecisionClass::DataLoss => "DATA_LOSS",
            DecisionClass::Routine => "ROUTINE",
        }
    }

    /// Parse the contract spelling. `None` for anything outside the six-value vocabulary.
    pub fn parse(value: &str) -> Option<Self> {
        DecisionClass::ALL
            .into_iter()
            .find(|class| class.as_str() == value)
    }

    /// Whether this class marks a material decision point.
    ///
    /// DEC-052 and `mode-selection.schema.json` both define a material decision point as one whose class is
    /// not `ROUTINE`. Kept as one predicate rather than an inline comparison so the definition has a single
    /// home.
    pub fn is_material(self) -> bool {
        self != DecisionClass::Routine
    }
}

impl std::fmt::Display for DecisionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Deliberation depth.
///
/// Ordered `Solo < Review < Full`, which is the order DEC-052's escalation moves in: upward only,
/// `SOLO -> REVIEW -> FULL`, and only between rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CouncilMode {
    /// A record and no round.
    Solo,
    /// A round with a single independent reviewer.
    Review,
    /// A round requiring the full participant content.
    Full,
}

impl CouncilMode {
    /// Every mode, weakest first.
    pub const ALL: [CouncilMode; 3] = [CouncilMode::Solo, CouncilMode::Review, CouncilMode::Full];

    /// The contract spelling, as stored in `council_mode_selections.mode`.
    pub fn as_str(self) -> &'static str {
        match self {
            CouncilMode::Solo => "SOLO",
            CouncilMode::Review => "REVIEW",
            CouncilMode::Full => "FULL",
        }
    }

    /// Parse the contract spelling.
    pub fn parse(value: &str) -> Option<Self> {
        CouncilMode::ALL
            .into_iter()
            .find(|mode| mode.as_str() == value)
    }

    /// Whether this mode opens a round.
    ///
    /// `SOLO` persists a selection record and opens no round; the other two both traverse the full
    /// eight-state spine, because DEC-052 forbids a phase-skipping round without a new off-spine edge.
    pub fn requires_round(self) -> bool {
        self != CouncilMode::Solo
    }

    /// The stronger of two modes. Escalation is upward only, so this is the only combination offered.
    pub fn escalate_to(self, other: CouncilMode) -> CouncilMode {
        if other > self {
            other
        } else {
            self
        }
    }
}

impl std::fmt::Display for CouncilMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who supplied the mode that was recorded.
///
/// `NONE` is not "no mode recorded" - the mode is always recorded. It means the mode was computed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideSource {
    /// The computed mode was recorded unchanged.
    None,
    /// An explicit user override replaced the computed mode.
    User,
}

impl OverrideSource {
    /// The contract spelling, as stored in `council_mode_selections.override_source`.
    pub fn as_str(self) -> &'static str {
        match self {
            OverrideSource::None => "NONE",
            OverrideSource::User => "USER",
        }
    }
}

/// An explicit user override of a computed mode.
///
/// DEC-013 makes the user the authority, so this carries no "reason" and no justification: a user does not
/// have to argue with the controller. What it cannot do is be silent, which is why the override is recorded
/// and reported in the selection's reasons.
///
/// Constructing this is a user-authority act. An agent cannot reach it through the MCF-v2 envelope: no
/// envelope or payload-schema field carries a mode, which is the structural half of "an agent can never
/// supply or influence a mode".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserOverride {
    /// The mode the user chose. May be higher or lower than the computed one.
    pub mode: CouncilMode,
    /// Who supplied it. Only [`OverrideSource::User`] is accepted by [`select_mode`].
    pub source: OverrideSource,
}

impl UserOverride {
    /// An override from the user.
    pub fn by_user(mode: CouncilMode) -> Self {
        UserOverride {
            mode,
            source: OverrideSource::User,
        }
    }
}

/// How far a decision's consequences reach.
///
/// `affected_file_count` and `crosses_workspace_boundary` are supplied by the controller's own scope and
/// workspace facts, never estimated by an agent: a blast radius an agent could inflate or deflate would
/// make the mode an agent choice by another name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastRadius {
    /// Authorized scope roots the decision touches. Never empty for a persisted record, which the contract
    /// states as `minItems: 1`.
    pub scope_roots: Vec<String>,
    /// Files the decision is expected to change, counted from the task graph and workspace facts.
    pub affected_file_count: u64,
    /// Whether the change crosses an authorized workspace boundary, which is the strongest available signal
    /// because it needs approval that an in-boundary change does not.
    pub crosses_workspace_boundary: bool,
}

impl BlastRadius {
    /// A blast radius for `scope_roots`.
    ///
    /// The roots are canonicalized here - sorted and de-duplicated - so the struct's invariant is that it
    /// holds the canonical form. That matters twice: the mode must not depend on the order a caller happened
    /// to list its roots in, and the selection id is derived from them, so two spellings of one scope set
    /// would otherwise mint two ids for one evaluation.
    pub fn new(
        scope_roots: &[&str],
        affected_file_count: u64,
        crosses_workspace_boundary: bool,
    ) -> Self {
        let distinct: BTreeSet<&str> = scope_roots.iter().copied().collect();
        BlastRadius {
            scope_roots: distinct.into_iter().map(str::to_string).collect(),
            affected_file_count,
            crosses_workspace_boundary,
        }
    }

    /// The scope roots in canonical form.
    ///
    /// Every constructor already canonicalizes, so this is the stored list; it is a function rather than a
    /// bare field access so a caller never has to know whether the value it holds came from `new` or from a
    /// struct literal.
    pub fn canonical_scope_roots(&self) -> Vec<String> {
        let set: BTreeSet<&String> = self.scope_roots.iter().collect();
        set.into_iter().cloned().collect()
    }
}

/// Everything the selector reads besides the thresholds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeInputs {
    pub decision_class: DecisionClass,
    pub blast_radius: BlastRadius,
    /// How many validation runs on this decision point failed before now.
    pub prior_validation_failures: u64,
    /// How many disputes are open against it.
    pub open_disputes: u64,
}

impl ModeInputs {
    /// Whether this is a material decision point. See [`DecisionClass::is_material`].
    pub fn is_material(&self) -> bool {
        self.decision_class.is_material()
    }
}

/// Selection thresholds, passed in as data.
///
/// Field names follow `council-policies.json:selection_thresholds` so the configuration file and this
/// struct are readable side by side. `routine_class_max_mode` is a ceiling on what the *computed* mode may
/// reach for a `ROUTINE` class; it is not a floor on the user's authority and never constrains an override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeThresholds {
    pub blast_radius_file_count_review: u64,
    pub blast_radius_file_count_full: u64,
    pub prior_validation_failures_review: u64,
    pub prior_validation_failures_full: u64,
    pub open_disputes_review: u64,
    pub open_disputes_full: u64,
    pub routine_class_default_mode: CouncilMode,
    pub routine_class_max_mode: CouncilMode,
    pub high_risk_classes_default_mode: CouncilMode,
    /// Classes treated as high risk. Membership is data, not a `match` over the enum, so configuration can
    /// move a class without a code change.
    pub high_risk_classes: Vec<DecisionClass>,
}

impl ModeThresholds {
    /// Whether `class` is configured as high risk.
    pub fn is_high_risk(&self, class: DecisionClass) -> bool {
        self.high_risk_classes.contains(&class)
    }
}

/// A controller-produced mode selection record.
///
/// This is the value `mode-selection.schema.json` validates and `council_mode_selections` stores. It is the
/// record, not the decision: nothing in this crate reads a mode selection to authorize anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeSelection {
    /// The computed mode, or the user's override.
    pub mode: CouncilMode,
    /// The mode computed from the inputs, before any override. Retained so a superseding record can show
    /// what the controller would have chosen, and so an override's direction is visible without re-running
    /// the selector against thresholds that may since have changed.
    pub computed_mode: CouncilMode,
    /// Every input that contributed, in a stable order.
    pub reasons: Vec<String>,
    /// Whether the recorded mode came from the controller or from the user.
    pub override_source: OverrideSource,
    /// The algorithm version.
    pub selector_version: String,
    /// The inputs, echoed so the record is self-describing.
    pub inputs: ModeInputs,
}

impl ModeSelection {
    /// Whether the recorded mode is a round-requiring mode.
    pub fn requires_round(&self) -> bool {
        self.mode.requires_round()
    }

    /// Whether an explicit user override replaced the computed mode.
    pub fn was_overridden(&self) -> bool {
        self.override_source == OverrideSource::User
    }

    /// The id this record would be persisted under, derived from its inputs.
    ///
    /// Derivation is deliberate. A random or clock-seeded id would name the same evaluation differently on
    /// two runs, so the contract's "same inputs, same record" property - which is what makes a selection
    /// auditable - would not hold. The digest covers the inputs, the override, the recorded mode and the
    /// selector version, so a record can never collide with the escalation it supersedes.
    ///
    /// This is identity, not security. Uniqueness is enforced by the table's PRIMARY KEY and only
    /// collision-resistance is required, which is the same reasoning `crates/core` records for its ids.
    pub fn selection_id(&self, project_id: &str, round_id: Option<&str>) -> String {
        let mut seed = String::new();
        push_field(&mut seed, project_id);
        push_field(&mut seed, round_id.unwrap_or(""));
        push_field(&mut seed, self.inputs.decision_class.as_str());
        push_field(
            &mut seed,
            &self.inputs.blast_radius.affected_file_count.to_string(),
        );
        push_field(
            &mut seed,
            if self.inputs.blast_radius.crosses_workspace_boundary {
                "1"
            } else {
                "0"
            },
        );
        for root in self.inputs.blast_radius.canonical_scope_roots() {
            push_field(&mut seed, &root);
        }
        push_field(
            &mut seed,
            &self.inputs.prior_validation_failures.to_string(),
        );
        push_field(&mut seed, &self.inputs.open_disputes.to_string());
        push_field(&mut seed, self.mode.as_str());
        push_field(&mut seed, self.override_source.as_str());
        push_field(&mut seed, &self.selector_version);

        format!("mds_{}", fnv1a_hex(&seed))
    }

    /// Build the storage request for this selection.
    ///
    /// `_clock` is the injected clock. It is unused because the record's identity and content are both
    /// derived from its inputs, but it stays in the signature: the persisted `created_at` is a caller-supplied
    /// timestamp, and taking the clock here is what makes it impossible for a caller to reach the system
    /// clock by accident on the path that writes this record.
    pub fn to_storage(
        &self,
        _clock: &dyn Clock,
        project_id: &str,
        round_id: Option<&str>,
        created_at: &str,
        supersedes_selection_id: Option<&str>,
    ) -> mayasaba_storage::NewCouncilModeSelection {
        mayasaba_storage::NewCouncilModeSelection {
            selection_id: self.selection_id(project_id, round_id),
            project_id: project_id.to_string(),
            round_id: round_id.map(|id| id.to_string()),
            decision_class: self.inputs.decision_class.as_str().to_string(),
            mode: self.mode.as_str().to_string(),
            blast_radius: mayasaba_storage::CouncilBlastRadius {
                scope_roots: self.inputs.blast_radius.canonical_scope_roots(),
                affected_file_count: self.inputs.blast_radius.affected_file_count,
                crosses_workspace_boundary: self.inputs.blast_radius.crosses_workspace_boundary,
            },
            prior_validation_failures: self.inputs.prior_validation_failures,
            open_disputes: self.inputs.open_disputes,
            reasons: self.reasons.clone(),
            selector_version: self.selector_version.clone(),
            override_source: self.override_source.as_str().to_string(),
            supersedes_selection_id: supersedes_selection_id.map(|id| id.to_string()),
            created_at: created_at.to_string(),
        }
    }
}

/// Compute the mode for a decision point, with the reasons that produced it.
///
/// `user_override` is `None` for every agent-reachable path. `thresholds` is configuration. Both the
/// recorded mode and the pre-override `computed_mode` are returned so an override is never silent.
pub fn select_mode(
    inputs: &ModeInputs,
    user_override: Option<UserOverride>,
    thresholds: &ModeThresholds,
    selector_version: &str,
) -> ModeSelection {
    let computed = compute_mode(inputs, thresholds);

    // Only a `USER` source is honoured. Any other source is treated as no override rather than as an error,
    // because a non-user source cannot occur through an agent-reachable path and refusing would turn an
    // unreachable state into a failure mode with no owner.
    let honored = user_override.filter(|attempt| attempt.source == OverrideSource::User);

    match honored {
        Some(attempt) => ModeSelection {
            mode: attempt.mode,
            computed_mode: computed.mode,
            reasons: override_reasons(&attempt, &computed, inputs),
            override_source: OverrideSource::User,
            selector_version: selector_version.to_string(),
            inputs: inputs.clone(),
        },
        None => ModeSelection {
            mode: computed.mode,
            computed_mode: computed.mode,
            reasons: computed.reasons,
            override_source: OverrideSource::None,
            selector_version: selector_version.to_string(),
            inputs: inputs.clone(),
        },
    }
}

/// The controller's own computation, before any override.
fn compute_mode(inputs: &ModeInputs, thresholds: &ModeThresholds) -> ModeSelection {
    let mut reasons: Vec<String> = Vec::new();

    // Step 1: the class. A high-risk class defaults to the configured high-risk mode. A `ROUTINE` class
    // defaults to the routine default. Every other class is material at a depth the policy artifact does not
    // name, so it starts at REVIEW - the decision is material by DEC-052's definition and cannot be left at
    // the depth configured for decisions that are not.
    let mut mode = if thresholds.is_high_risk(inputs.decision_class) {
        reasons.push(format!(
            "decision class {} is configured high risk, defaulting to {}",
            inputs.decision_class,
            thresholds.high_risk_classes_default_mode.as_str()
        ));
        thresholds.high_risk_classes_default_mode
    } else if inputs.decision_class == DecisionClass::Routine {
        reasons.push(format!(
            "decision class ROUTINE is not a material decision point and defaults to {}",
            thresholds.routine_class_default_mode.as_str()
        ));
        thresholds.routine_class_default_mode
    } else {
        reasons.push(format!(
            "decision class {} is material and is not configured high risk, defaulting to REVIEW",
            inputs.decision_class
        ));
        CouncilMode::Review
    };

    // Step 2: class-independent escalation triggers, each compared to its configured threshold. A trigger
    // fires at its threshold, so a configuration of `1` means "one prior failure is enough" and a
    // configuration of `2` means "wait for the second".
    if inputs.blast_radius.crosses_workspace_boundary {
        mode = mode.escalate_to(CouncilMode::Full);
        reasons.push(
            "blast radius crosses an authorized workspace boundary, which requires full deliberation"
                .to_string(),
        );
    }
    if threshold_reached(
        inputs.blast_radius.affected_file_count,
        thresholds.blast_radius_file_count_full,
    ) {
        mode = mode.escalate_to(CouncilMode::Full);
        reasons.push(format!(
            "blast radius of {} affected files reaches the review-to-full threshold {}",
            inputs.blast_radius.affected_file_count, thresholds.blast_radius_file_count_full
        ));
    } else if threshold_reached(
        inputs.blast_radius.affected_file_count,
        thresholds.blast_radius_file_count_review,
    ) {
        mode = mode.escalate_to(CouncilMode::Review);
        reasons.push(format!(
            "blast radius of {} affected files reaches the review threshold {}",
            inputs.blast_radius.affected_file_count, thresholds.blast_radius_file_count_review
        ));
    }

    if threshold_reached(
        inputs.prior_validation_failures,
        thresholds.prior_validation_failures_full,
    ) {
        mode = mode.escalate_to(CouncilMode::Full);
        reasons.push(format!(
            "{} prior validation failures reach the full threshold {}",
            inputs.prior_validation_failures, thresholds.prior_validation_failures_full
        ));
    } else if threshold_reached(
        inputs.prior_validation_failures,
        thresholds.prior_validation_failures_review,
    ) {
        mode = mode.escalate_to(CouncilMode::Review);
        reasons.push(format!(
            "{} prior validation failures reach the review threshold {}",
            inputs.prior_validation_failures, thresholds.prior_validation_failures_review
        ));
    }

    if threshold_reached(inputs.open_disputes, thresholds.open_disputes_full) {
        mode = mode.escalate_to(CouncilMode::Full);
        reasons.push(format!(
            "{} open disputes reach the full threshold {}",
            inputs.open_disputes, thresholds.open_disputes_full
        ));
    } else if threshold_reached(inputs.open_disputes, thresholds.open_disputes_review) {
        mode = mode.escalate_to(CouncilMode::Review);
        reasons.push(format!(
            "{} open disputes reach the review threshold {}",
            inputs.open_disputes, thresholds.open_disputes_review
        ));
    }

    // Step 3: the routine ceiling, applied last so it caps everything above. It applies to the `ROUTINE`
    // class, which is what the threshold is named for: a cap on decisions that are not material. A material
    // class that is not high risk starts at REVIEW regardless, because the routine default and the routine
    // ceiling are about routine decisions.
    //
    // The cap bounds the computation only. A user override is applied after this and is never capped.
    if inputs.decision_class == DecisionClass::Routine && mode > thresholds.routine_class_max_mode {
        mode = thresholds.routine_class_max_mode;
        reasons.push(format!(
            "ROUTINE is capped at {} by configuration",
            thresholds.routine_class_max_mode.as_str()
        ));
    }

    // A material decision point must never compute to SOLO, which opens no round. SOLO is a legitimate mode
    // for a material decision when the *user* chooses it (DEC-013 and the configuration rule both say a
    // user override may lower rigor), but the controller must not swallow a material decision at a depth the
    // configuration names for decisions that are not material. This fires only when a high-risk class is
    // configured with a SOLO default, so the shipped configuration never reaches it.
    if inputs.is_material() && mode == CouncilMode::Solo {
        mode = CouncilMode::Review;
        reasons.push(format!(
            "class {} is material and the configured default opens no round; raised to REVIEW",
            inputs.decision_class
        ));
    }

    ModeSelection {
        mode,
        computed_mode: mode,
        reasons,
        override_source: OverrideSource::None,
        selector_version: DEFAULT_SELECTOR_VERSION.to_string(),
        inputs: inputs.clone(),
    }
}

/// The reasons list for an overridden selection: the computed ones, the override, and explicitly not the
/// computed mode as the recorded one.
fn override_reasons(
    attempt: &UserOverride,
    computed: &ModeSelection,
    inputs: &ModeInputs,
) -> Vec<String> {
    let direction = if attempt.mode > computed.mode {
        "raised"
    } else if attempt.mode < computed.mode {
        "lowered"
    } else {
        "confirmed"
    };

    let mut reasons = computed.reasons.clone();
    reasons.push(format!(
        "user override {direction} the mode from {} to {} (DEC-013: the user is the authority)",
        computed.mode.as_str(),
        attempt.mode.as_str()
    ));
    if attempt.mode == CouncilMode::Solo {
        reasons.push(
            "SOLO by user override opens no round; the selection record is still persisted (DEC-052)"
                .to_string(),
        );
    }
    if !inputs.is_material() {
        reasons.push("decision class ROUTINE is not a material decision point".to_string());
    }
    reasons
}

/// Whether `value` reaches `threshold`.
///
/// A configured threshold of zero is treated as "never fires" rather than "always fires": a zero threshold
/// is how a configuration disables a trigger, and reading it as always-on would make a threshold of 0 mean
/// FULL for every decision, which no configuration intends.
fn threshold_reached(value: u64, threshold: u64) -> bool {
    threshold > 0 && value >= threshold
}

/// Append a length-prefixed field, so no concatenation of two different inputs can produce one digest.
fn push_field(seed: &mut String, value: &str) {
    seed.push_str(&value.len().to_string());
    seed.push(':');
    seed.push_str(value);
    seed.push('|');
}

/// FNV-1a over the UTF-8 bytes, formatted as 16 hex digits.
///
/// Dependency-free and not cryptographic; see [`ModeSelection::selection_id`] for why that is enough here.
fn fnv1a_hex(seed: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;

    /// The shipped `council-policies.json` thresholds, so the tests exercise the configuration the product
    /// actually ships rather than a convenient invention.
    fn shipped_thresholds() -> ModeThresholds {
        ModeThresholds {
            blast_radius_file_count_review: 10,
            blast_radius_file_count_full: 50,
            prior_validation_failures_review: 1,
            prior_validation_failures_full: 2,
            open_disputes_review: 1,
            open_disputes_full: 1,
            routine_class_default_mode: CouncilMode::Solo,
            routine_class_max_mode: CouncilMode::Review,
            high_risk_classes_default_mode: CouncilMode::Full,
            high_risk_classes: vec![
                DecisionClass::Irreversible,
                DecisionClass::Security,
                DecisionClass::DataLoss,
            ],
        }
    }

    fn inputs(class: DecisionClass) -> ModeInputs {
        ModeInputs {
            decision_class: class,
            blast_radius: BlastRadius::new(&["crates/council"], 3, false),
            prior_validation_failures: 0,
            open_disputes: 0,
        }
    }

    fn select(inputs: &ModeInputs, thresholds: &ModeThresholds) -> ModeSelection {
        select_mode(inputs, None, thresholds, DEFAULT_SELECTOR_VERSION)
    }

    #[test]
    fn the_same_inputs_always_produce_the_same_selection() {
        let first = select(&inputs(DecisionClass::Architecture), &shipped_thresholds());
        let second = select(&inputs(DecisionClass::Architecture), &shipped_thresholds());
        assert_eq!(
            first, second,
            "selection must be a pure function of its inputs"
        );

        // Scope root order is not an input. Two spellings of one scope set must agree, because the
        // controller's scope list order is not a fact about the decision.
        let mut reordered = inputs(DecisionClass::Architecture);
        reordered.blast_radius = BlastRadius::new(&["crates/storage", "crates/council"], 3, false);
        let mut other_order = inputs(DecisionClass::Architecture);
        other_order.blast_radius =
            BlastRadius::new(&["crates/council", "crates/storage"], 3, false);
        assert_eq!(
            select(&reordered, &shipped_thresholds()),
            select(&other_order, &shipped_thresholds())
        );
        assert_eq!(
            select(&reordered, &shipped_thresholds()).selection_id("prj_1", None),
            select(&other_order, &shipped_thresholds()).selection_id("prj_1", None)
        );
    }

    #[test]
    fn reasons_name_every_input_that_contributed() {
        let mut blurry = inputs(DecisionClass::StackTechnology);
        blurry.blast_radius = BlastRadius::new(&["crates/council"], 12, true);
        blurry.prior_validation_failures = 1;
        blurry.open_disputes = 1;

        let selection = select(&blurry, &shipped_thresholds());
        assert_eq!(selection.mode, CouncilMode::Full);

        let reported = selection.reasons.join("\n");
        for expected in [
            "decision class STACK_TECHNOLOGY",
            "crosses an authorized workspace boundary",
            "12 affected files",
            "review threshold 10",
            "1 prior validation failures",
            "review threshold 1",
            "1 open disputes",
            "full threshold 1",
        ] {
            assert!(
                reported.contains(expected),
                "reasons omitted `{expected}`:\n{reported}"
            );
        }
        assert!(
            !selection.reasons.is_empty(),
            "the contract requires at least one reason"
        );

        // A blast radius that reaches the full threshold names that threshold, and the review threshold it
        // also passes is not reported as though it were the reason.
        let mut wide = inputs(DecisionClass::StackTechnology);
        wide.blast_radius = BlastRadius::new(&["crates/council"], 60, false);
        let wide_reported = select(&wide, &shipped_thresholds()).reasons.join("\n");
        assert!(
            wide_reported.contains("review-to-full threshold 50"),
            "{wide_reported}"
        );
    }

    #[test]
    fn a_user_override_is_recorded_and_may_lower_rigor() {
        let security = inputs(DecisionClass::Security);
        let computed = select(&security, &shipped_thresholds());
        assert_eq!(
            computed.mode,
            CouncilMode::Full,
            "a high-risk class defaults to FULL"
        );

        // The downward override the specification explicitly requires not to be refused.
        let lowered = select_mode(
            &security,
            Some(UserOverride::by_user(CouncilMode::Solo)),
            &shipped_thresholds(),
            DEFAULT_SELECTOR_VERSION,
        );
        assert_eq!(
            lowered.mode,
            CouncilMode::Solo,
            "a user may lower rigor; there is no floor"
        );
        assert_eq!(
            lowered.computed_mode,
            CouncilMode::Full,
            "the computed mode stays visible"
        );
        assert!(lowered.was_overridden());
        assert_eq!(lowered.override_source, OverrideSource::User);
        assert!(lowered
            .reasons
            .iter()
            .any(|reason| reason.contains("user override lowered")));
        assert!(
            lowered
                .reasons
                .iter()
                .any(|reason| reason.contains("DEC-013")),
            "the override's authority must be named in the record"
        );

        // And the upward direction.
        let routine = inputs(DecisionClass::Routine);
        assert_eq!(
            select(&routine, &shipped_thresholds()).mode,
            CouncilMode::Solo
        );
        let raised = select_mode(
            &routine,
            Some(UserOverride::by_user(CouncilMode::Full)),
            &shipped_thresholds(),
            DEFAULT_SELECTOR_VERSION,
        );
        assert_eq!(raised.mode, CouncilMode::Full);
        assert!(raised
            .reasons
            .iter()
            .any(|reason| reason.contains("user override raised")));
    }

    #[test]
    fn an_override_that_is_not_from_the_user_is_not_honoured() {
        let routine = inputs(DecisionClass::Routine);
        let selection = select_mode(
            &routine,
            Some(UserOverride {
                mode: CouncilMode::Full,
                source: OverrideSource::None,
            }),
            &shipped_thresholds(),
            DEFAULT_SELECTOR_VERSION,
        );
        assert_eq!(selection.mode, CouncilMode::Solo);
        assert_eq!(selection.override_source, OverrideSource::None);
    }

    #[test]
    fn the_thresholds_are_data_not_constants() {
        let strict = ModeThresholds {
            blast_radius_file_count_review: 1,
            blast_radius_file_count_full: 2,
            ..shipped_thresholds()
        };
        let mut small = inputs(DecisionClass::Architecture);
        small.blast_radius = BlastRadius::new(&["crates/council"], 1, false);

        // One affected file is below the shipped review threshold of 10 and at the modified one of 1.
        let shipped = select(&small, &shipped_thresholds());
        assert_eq!(
            shipped.mode,
            CouncilMode::Review,
            "a material class defaults to REVIEW"
        );
        assert!(
            !shipped
                .reasons
                .iter()
                .any(|reason| reason.contains("affected files")),
            "one file must not reach the shipped review threshold: {:?}",
            shipped.reasons
        );
        let changed = select(&small, &strict);
        assert_eq!(changed.mode, CouncilMode::Review);
        assert!(changed
            .reasons
            .iter()
            .any(|reason| reason.contains("threshold 1")));

        // A different default mode for the routine class really does change the outcome for a routine
        // decision, and a different ceiling really does bound it.
        let routine_more_thorough = ModeThresholds {
            routine_class_default_mode: CouncilMode::Review,
            routine_class_max_mode: CouncilMode::Full,
            ..shipped_thresholds()
        };
        assert_eq!(
            select(&inputs(DecisionClass::Routine), &shipped_thresholds()).mode,
            CouncilMode::Solo
        );
        assert_eq!(
            select(&inputs(DecisionClass::Routine), &routine_more_thorough).mode,
            CouncilMode::Review
        );

        // Moving a class into or out of the high-risk set is configuration too.
        let architecture_is_high_risk = ModeThresholds {
            high_risk_classes: vec![DecisionClass::Architecture],
            ..shipped_thresholds()
        };
        assert_eq!(
            select(
                &inputs(DecisionClass::Architecture),
                &architecture_is_high_risk
            )
            .mode,
            CouncilMode::Full
        );
        let security_is_not_high_risk = ModeThresholds {
            high_risk_classes: vec![DecisionClass::DataLoss],
            ..shipped_thresholds()
        };
        let demoted = select(&inputs(DecisionClass::Security), &security_is_not_high_risk);
        assert_eq!(
            demoted.mode,
            CouncilMode::Review,
            "a demoted material class falls to the material default"
        );
        assert!(demoted
            .reasons
            .iter()
            .any(|reason| reason.contains("not configured high risk")));
    }

    #[test]
    fn a_zero_threshold_disables_its_trigger_rather_than_firing_always() {
        let disabled = ModeThresholds {
            prior_validation_failures_review: 0,
            prior_validation_failures_full: 0,
            open_disputes_review: 0,
            open_disputes_full: 0,
            blast_radius_file_count_review: 0,
            blast_radius_file_count_full: 0,
            ..shipped_thresholds()
        };
        let mut noisy = inputs(DecisionClass::Architecture);
        noisy.blast_radius = BlastRadius::new(&["crates/council"], 9_999, false);
        noisy.prior_validation_failures = 7;
        noisy.open_disputes = 7;

        let selection = select(&noisy, &disabled);
        assert_eq!(
            selection.mode,
            CouncilMode::Review,
            "only the class default is left"
        );
        assert!(!selection
            .reasons
            .iter()
            .any(|reason| reason.contains("threshold 0")));
    }

    #[test]
    fn a_material_class_never_computes_to_solo() {
        // A configuration that would send a material decision to SOLO - here by naming a SOLO high-risk
        // default, which is the only path to it - must not silently record a material decision with no round.
        let reckless = ModeThresholds {
            routine_class_default_mode: CouncilMode::Solo,
            routine_class_max_mode: CouncilMode::Solo,
            high_risk_classes_default_mode: CouncilMode::Solo,
            high_risk_classes: vec![DecisionClass::Irreversible, DecisionClass::Security],
            ..shipped_thresholds()
        };
        for class in DecisionClass::ALL {
            let selection = select(&inputs(class), &reckless);
            if class.is_material() {
                assert!(
                    selection.mode.requires_round(),
                    "{class} is material and must not compute to SOLO: {:?}",
                    selection.reasons
                );
                if reckless.is_high_risk(class) {
                    assert_eq!(selection.mode, CouncilMode::Review);
                    assert!(selection
                        .reasons
                        .iter()
                        .any(|reason| reason.contains("opens no round")));
                }
            } else {
                assert_eq!(selection.mode, CouncilMode::Solo, "ROUTINE stays SOLO");
                assert!(!selection.requires_round());
            }
        }
    }

    #[test]
    fn a_routine_class_is_capped_by_configuration() {
        let mut wide = inputs(DecisionClass::Routine);
        wide.blast_radius = BlastRadius::new(&["crates/council"], 500, true);
        wide.open_disputes = 4;
        let selection = select(&wide, &shipped_thresholds());
        assert_eq!(
            selection.mode,
            CouncilMode::Review,
            "ROUTINE is capped at REVIEW by configuration"
        );
        assert!(selection
            .reasons
            .iter()
            .any(|reason| reason.contains("ROUTINE is capped at REVIEW")));
        assert_eq!(selection.inputs, wide, "the inputs are echoed unchanged");
    }

    #[test]
    fn solo_opens_no_round_and_the_others_do() {
        assert!(!select(&inputs(DecisionClass::Routine), &shipped_thresholds()).requires_round());
        assert!(
            select(&inputs(DecisionClass::Architecture), &shipped_thresholds()).requires_round()
        );
        assert!(!CouncilMode::Solo.requires_round());
        assert!(CouncilMode::Review.requires_round());
        assert!(CouncilMode::Full.requires_round());
    }

    #[test]
    fn the_selection_id_is_stable_and_distinguishes_its_inputs() {
        let clock = FixedClock::new("2026-10-04T00:00:00Z");
        let base = select(&inputs(DecisionClass::Architecture), &shipped_thresholds());
        let stored = base.to_storage(&clock, "prj_a", Some("rnd_1"), "2026-10-04T00:00:00Z", None);

        assert_eq!(
            stored.selection_id,
            base.selection_id("prj_a", Some("rnd_1"))
        );
        assert!(stored.selection_id.starts_with("mds_"));
        assert_eq!(stored.mode, "REVIEW");
        assert_eq!(stored.override_source, "NONE");
        assert_eq!(stored.round_id.as_deref(), Some("rnd_1"));

        // A different project, round, escalation level or override must not collide with it.
        assert_ne!(
            stored.selection_id,
            base.selection_id("prj_b", Some("rnd_1"))
        );
        assert_ne!(stored.selection_id, base.selection_id("prj_a", None));
        let escalated = select(&inputs(DecisionClass::Security), &shipped_thresholds());
        assert_ne!(
            stored.selection_id,
            escalated.selection_id("prj_a", Some("rnd_1"))
        );
        let overridden = select_mode(
            &inputs(DecisionClass::Architecture),
            Some(UserOverride::by_user(CouncilMode::Solo)),
            &shipped_thresholds(),
            DEFAULT_SELECTOR_VERSION,
        );
        assert_ne!(
            stored.selection_id,
            overridden.selection_id("prj_a", Some("rnd_1"))
        );
    }

    #[test]
    fn the_decision_class_vocabulary_is_exactly_the_contracts() {
        assert_eq!(DecisionClass::ALL.len(), 6);
        for class in DecisionClass::ALL {
            assert_eq!(DecisionClass::parse(class.as_str()), Some(class));
            assert_eq!(class.to_string(), class.as_str());
        }
        assert_eq!(DecisionClass::parse("MAYBE"), None);
        assert!(!DecisionClass::Routine.is_material());
        assert!(DecisionClass::DataLoss.is_material());
    }
}
