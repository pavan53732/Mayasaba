//! Mayasaba council decision quality (DEC-052).
//!
//! This crate is the deterministic, controller-owned half of council deliberation: the facts a round's
//! quality rests on. It is pure, synchronous domain logic. It spawns no agent, touches no bus, executes no
//! process, opens no socket, starts no thread, holds no global mutable state and never reads the system
//! clock directly - time arrives through the [`Clock`] parameter, so every function here is deterministic
//! for equal inputs.
//!
//! What it computes, and why the controller computes it rather than accepting it:
//!
//! * [`mode`] - `SOLO`/`REVIEW`/`FULL` selected from structured inputs and configured thresholds. No agent
//!   may select or downgrade a mode, and no LLM chooses one.
//! * [`evidence`] - `ASSUMPTION < CITED < VERIFIED` graded from resolvable references. An agent-supplied
//!   grade is not representable: no function here accepts one.
//! * [`convergence`] - whether a round may seal `CONVERGED`. This is a precondition on sealing, not a
//!   replacement for the fixpoint predicate, and it adds no `outcome_type`.
//! * [`lineage`] - corroboration counted in lineage groups derived from static adapter facts, never from
//!   agent self-report, so Kilo and OpenCode count once.
//! * [`roles`] - deterministic, rotating `PROPOSER`/`SKEPTIC`/`VERIFIER` assignment. A role is framing,
//!   never authority, so nothing in this crate turns a role into a permission.
//! * [`synthesis`] - coverage and independent-review checks on a chair's `SYNTHESIS`.
//! * [`budget`] - rounds, wall-clock, tokens and spikes against policy caps, with paused time excluded and
//!   an unreported token count recorded as unavailable rather than estimated.
//! * [`outcome`] - append-only decision outcome records derived only from controller facts.
//!
//! Every SQL statement lives in `mayasaba-storage`; this crate holds no SQL string and reaches durable
//! state only through that crate's typed APIs.

pub mod budget;
pub mod clock;
pub mod convergence;
pub mod error;
pub mod evidence;
pub mod lineage;
pub mod mode;
pub mod outcome;
pub mod roles;
pub mod synthesis;

pub use budget::{
    available_now, compute_consumption, exhaustion, parse_rfc3339_utc, rfc3339_from_epoch_seconds,
    BudgetCaps, BudgetDimension, BudgetEntry, BudgetEntryKind, BudgetExhaustion,
    BudgetExhaustionOutcome, BudgetLedger, BudgetPolicy, BudgetSnapshot, DimensionState,
    TokenAvailability,
};
pub use clock::{Clock, FixedClock, SystemClock};
pub use convergence::{
    can_seal_converged, ConvergencePrecondition, ConvergenceRefusal, ConvergenceVerdict,
};
pub use error::{CouncilError, Result};
pub use evidence::{
    grade_claim, grade_position, Claim, ClaimGrade, ClaimGradeBasis, EvidenceResolver, GradeReason,
    Position, PositionGrade, ReferenceResolution,
};
pub use lineage::{
    adapter_lineage, count_lineage_groups, lineage_groups_for, share_codebase, Corroboration,
    LineageGroup, LineageSource, ADAPTER_LINEAGE, DEFAULT_MINIMUM_LINEAGE_GROUPS,
};
pub use mode::{
    select_mode, BlastRadius, CouncilMode, DecisionClass, ModeInputs, ModeSelection,
    ModeThresholds, OverrideSource, UserOverride, DEFAULT_SELECTOR_VERSION,
};
pub use outcome::{
    CouncilDecisionOutcome, OutcomeAgentLink, OutcomeRecordBuilder, OutcomeSource, OutcomeStatus,
    OutcomeStore,
};
pub use roles::{
    assign_round_roles, RoleAssignment, RoleKind, RoundRole, RoundRoleRecord, RoundRoles,
    ASSIGNMENT_REASON_ROTATION, LINEAGE_UNAVOIDABLE,
};
pub use synthesis::{
    check_reviewer_eligibility, check_synthesis_coverage, extra_citations, CoverageGap,
    ReviewerEligibility, ReviewerRefusal, SynthesisCoverage, SynthesisPosition,
};

/// The crate's own name, used by the workspace manifest's boundary checks.
pub const CRATE_NAME: &str = "mayasaba-council";
