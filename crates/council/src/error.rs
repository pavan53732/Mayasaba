//! The council crate's structured error surface.
//!
//! Every variant carries a `code` whose value is a key of `schemas/error-v1/registry.json` wherever an
//! existing key genuinely fits, because the registry is the single canonical error vocabulary (DEC-052 made
//! decision quality controller-computed, not agent-claimed, and the registry is how a failure is named
//! across the bus, the Tauri bridge and the Control Room).
//!
//! Two registrations were considered and deliberately not invented, because AGENTS.md section 8 forbids
//! inventing contract vocabulary and this crate may not add a registry entry:
//!
//! * No key covers "a quality precondition refused a record" - the family that `HELD` without validation
//!   evidence, a missing supersession link and an unmeasurable budget dimension all belong to. They are
//!   reported as `SCHEMA_INVALID`, the registry key that already means "the value does not satisfy its
//!   declared contract", and the [`CouncilError::gap`] accessor names the missing key explicitly rather
//!   than pretending the mapping is exact.
//! * No key covers "an append-only record was mutated". That attempt is not representable in this crate's
//!   API at all (see [`crate::outcome`]), so it needs no code.

use mayasaba_storage::StorageError;

/// A refusal or failure produced by the council crate.
///
/// The variants are per-rule so a caller can react programmatically rather than by string matching, which
/// is the same shape `mayasaba_protocol::envelope::EnvelopeRejection` uses.
#[derive(Debug)]
pub enum CouncilError {
    /// A `HELD` outcome asserted the decision survived but carried no validation evidence reference.
    ///
    /// `HELD` is the only status claiming the decision held up, so it is the only one the record contract
    /// requires evidence for (`schemas/council-v1/decision-outcome.schema.json`).
    OutcomeMissingValidationEvidence {
        status: &'static str,
        code: &'static str,
    },
    /// An `AMENDED` or `REVERSED` outcome did not name the outcome it supersedes.
    ///
    /// Both statuses are derived from a change to an earlier record, so a record claiming one without a
    /// predecessor would describe an amendment to nothing.
    OutcomeMissingSupersededRecord {
        status: &'static str,
        code: &'static str,
    },
    /// An outcome link named no agent. A link with no subject attributes a stance to nobody.
    OutcomeLinkMissingAgent { index: usize, code: &'static str },
    /// A budget dimension could not be measured from the ledger, so no number is reported for it.
    ///
    /// A dimension lands here when its timestamps are not parseable RFC3339. The dimension is reported as
    /// unmeasurable rather than as zero, for the same reason tokens are `UNAVAILABLE` rather than estimated:
    /// a fabricated zero reads as a real measurement.
    ///
    /// [`crate::budget::exhaustion`] returns this condition as `BudgetDimension::Unmeasurable` rather than as
    /// an error, because an unmeasurable dimension is a fact a caller reports and not a failure of the
    /// calculation. This variant exists for a caller that would rather refuse to produce a snapshot than
    /// publish one with a gap in it.
    BudgetDimensionUnmeasurable { detail: String, code: &'static str },
    /// The storage layer refused the operation. The wrapped error is preserved rather than flattened.
    Storage {
        error: StorageError,
        code: &'static str,
    },
}

impl CouncilError {
    /// The error-registry key this failure reports under.
    pub fn code(&self) -> &'static str {
        match self {
            CouncilError::OutcomeMissingValidationEvidence { code, .. }
            | CouncilError::OutcomeMissingSupersededRecord { code, .. }
            | CouncilError::OutcomeLinkMissingAgent { code, .. }
            | CouncilError::BudgetDimensionUnmeasurable { code, .. }
            | CouncilError::Storage { code, .. } => code,
        }
    }

    /// The registry key this crate would have used if the registry declared it, or `None` when an existing
    /// key fits.
    ///
    /// This exists so a mapping that is convenient rather than exact is visible to a caller instead of
    /// hidden inside a `match` arm.
    pub fn gap(&self) -> Option<&'static str> {
        match self {
            CouncilError::OutcomeMissingValidationEvidence { .. } => {
                Some("OUTCOME_EVIDENCE_REQUIRED")
            }
            CouncilError::OutcomeMissingSupersededRecord { .. } => {
                Some("OUTCOME_SUPERSESSION_REQUIRED")
            }
            CouncilError::OutcomeLinkMissingAgent { .. } => Some("OUTCOME_LINK_SUBJECT_REQUIRED"),
            CouncilError::BudgetDimensionUnmeasurable { .. } => Some("BUDGET_UNMEASURABLE"),
            CouncilError::Storage { .. } => None,
        }
    }

    /// Convenience constructor for a refusal that has no registry key of its own.
    pub(crate) fn contract(status: &'static str, code: &'static str) -> CouncilError {
        CouncilError::OutcomeMissingValidationEvidence { status, code }
    }

    pub(crate) fn supersession(status: &'static str, code: &'static str) -> CouncilError {
        CouncilError::OutcomeMissingSupersededRecord { status, code }
    }

    pub(crate) fn link(index: usize, code: &'static str) -> CouncilError {
        CouncilError::OutcomeLinkMissingAgent { index, code }
    }

    /// The registry code used for every rule whose family the registry does not yet name.
    pub(crate) const CONTRACT_CODE: &'static str = "SCHEMA_INVALID";
}

impl std::fmt::Display for CouncilError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CouncilError::OutcomeMissingValidationEvidence { status, .. } => write!(
                f,
                "council outcome status `{status}` asserts the decision survived and requires a validation evidence reference"
            ),
            CouncilError::OutcomeMissingSupersededRecord { status, .. } => write!(
                f,
                "council outcome status `{status}` requires the id of the outcome record it supersedes"
            ),
            CouncilError::OutcomeLinkMissingAgent { index, .. } => {
                write!(f, "council outcome agent link {index} names no agent")
            }
            CouncilError::BudgetDimensionUnmeasurable { detail, .. } => {
                write!(f, "council budget dimension could not be measured: {detail}")
            }
            CouncilError::Storage { error, .. } => write!(f, "council storage operation failed: {error}"),
        }
    }
}

impl std::error::Error for CouncilError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CouncilError::Storage { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl From<StorageError> for CouncilError {
    fn from(error: StorageError) -> Self {
        // A storage failure is reported under the existing schema/database key rather than a new one: the
        // registry already distinguishes input failures (`SCHEMA_INVALID`) from policy and identity ones, and
        // no council-specific persistence key exists.
        CouncilError::Storage {
            error,
            code: CouncilError::CONTRACT_CODE,
        }
    }
}

/// The council crate's result type.
pub type Result<T> = std::result::Result<T, CouncilError>;
