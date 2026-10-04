//! Council decision-quality conformance tests over real SQLite state (DEC-052).
//!
//! These are the tests that need persistence: the pure logic is covered by the unit tests inside each
//! module, and what these add is that the controller's computed facts survive a round trip through
//! `crates/storage` into the six new tables and back out again, that the database's own constraints enforce
//! the same rules the domain enforces, and that no path here can update or delete an outcome record.
//!
//! Fixtures are inserted directly for the rows no owning service in this workspace can create yet (projects,
//! council sessions, rounds, agents, positions, decisions, evidence). Direct insertion is deliberate rather
//! than a mocking layer: a mocked database would prove nothing about the tables, and the foreign keys are
//! exactly what these tests need switched on.

use mayasaba_council::{
    assign_round_roles, can_seal_converged, check_synthesis_coverage, compute_consumption,
    count_lineage_groups, exhaustion, grade_position, select_mode, BlastRadius, BudgetDimension,
    BudgetLedger, Claim, ClaimGrade, ConvergencePrecondition, ConvergenceVerdict,
    CouncilDecisionOutcome, CouncilMode, DecisionClass, EvidenceResolver, FixedClock, ModeInputs,
    ModeSelection, ModeThresholds, OutcomeAgentLink, OutcomeRecordBuilder, OutcomeSource,
    OutcomeStatus, OutcomeStore, Position, ReferenceResolution, RoleKind, SynthesisPosition,
    UserOverride, DEFAULT_MINIMUM_LINEAGE_GROUPS, DEFAULT_SELECTOR_VERSION, LINEAGE_UNAVOIDABLE,
};
use mayasaba_storage::{
    CouncilClaimGradeBasis, CouncilClaimReference, NewCouncilClaimGrade, NewCouncilDecisionOutcome,
    NewCouncilRoundRole, Storage,
};

const PROJECT: &str = "prj_1";
const COUNCIL: &str = "cns_1";
const ROUND: &str = "rnd_1";
const NOW: &str = "2026-10-04T10:00:00Z";

/// A database with the rows the council tables reference.
fn database() -> Storage {
    let storage = Storage::open_in_memory().expect("open in-memory store");
    let conn = storage.conn();

    conn.execute(
        "INSERT INTO projects (project_id, name, local_path, phase, status, current_epoch, created_at, updated_at)
         VALUES (?1, 'Mayasaba', 'C:\\work', 'EXECUTION', 'ACTIVE', 0, ?2, ?2)",
        [PROJECT, NOW],
    )
    .expect("insert project");

    conn.execute(
        "INSERT INTO council_sessions (council_session_id, project_id, state, created_at)
         VALUES (?1, ?2, 'OPEN', ?3)",
        [COUNCIL, PROJECT, NOW],
    )
    .expect("insert council session");

    conn.execute(
        "INSERT INTO council_rounds (round_id, council_session_id, project_id, state, epoch, created_at)
         VALUES (?1, ?2, ?3, 'OPEN', 0, ?4)",
        [ROUND, COUNCIL, PROJECT, NOW],
    )
    .expect("insert round");

    for (agent_id, agent_type) in [
        ("agent-hermes", "HERMES_AGENT"),
        ("agent-kilo", "KILO_CODE"),
        ("agent-opencode", "OPEN_CODE"),
    ] {
        conn.execute(
            "INSERT INTO agents (agent_id, agent_type, executable, enabled, created_at, updated_at)
             VALUES (?1, ?2, 'agent.exe', 1, ?3, ?3)",
            [agent_id, agent_type, NOW],
        )
        .expect("insert agent");
    }

    conn.execute(
        "INSERT INTO decisions (decision_id, project_id, version, class, status, subject, decision_text, created_at)
         VALUES ('dec_1', ?1, 1, 'ARCHITECTURE', 'PROPOSED', 'Storage', 'SQLite is the source of truth', ?2)",
        [PROJECT, NOW],
    )
    .expect("insert decision");

    conn.execute(
        "INSERT INTO evidence (evidence_id, project_id, kind, source_json, created_at)
         VALUES ('ev_1', ?1, 'VALIDATION', '{}', ?2)",
        [PROJECT, NOW],
    )
    .expect("insert evidence");

    conn.execute(
        "INSERT INTO council_positions (position_id, round_id, agent_id, position_type, body_json, created_at)
         VALUES ('pos_a', ?1, 'agent-hermes', 'PROPOSAL', '{}', ?2)",
        [ROUND, NOW],
    )
    .expect("insert position");

    storage
}

/// The shipped `council-policies.json` thresholds.
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
        blast_radius: BlastRadius::new(&["crates/council"], 12, false),
        prior_validation_failures: 0,
        open_disputes: 0,
    }
}

/// A resolver over the evidence row the fixture inserted.
struct RepositoryFacts;

impl EvidenceResolver for RepositoryFacts {
    fn resolve(&self, reference: &str) -> ReferenceResolution {
        match reference {
            "evidence:ev_1" => ReferenceResolution::existing_fact(),
            "check:cargo-test" => ReferenceResolution::controller_executed_check(),
            _ => {
                ReferenceResolution::unresolvable(mayasaba_council::GradeReason::ReferenceNotFound)
            }
        }
    }
}

/// Persist a selection and read it back, which is the round trip every other test relies on.
fn round_trip(selection: &ModeSelection) -> mayasaba_storage::CouncilModeSelectionRecord {
    let mut storage = database();
    let clock = FixedClock::new(NOW);
    let request = selection.to_storage(&clock, PROJECT, Some(ROUND), NOW, None);
    storage
        .insert_council_mode_selection(&request)
        .expect("insert selection");

    let records = storage
        .list_council_mode_selections(PROJECT)
        .expect("list selections");
    assert_eq!(records.len(), 1);
    records
        .into_iter()
        .next()
        .unwrap_or_else(|| mayasaba_storage::CouncilModeSelectionRecord {
            selection_id: String::new(),
            project_id: String::new(),
            round_id: None,
            decision_class: String::new(),
            mode: String::new(),
            inputs_json: String::new(),
            reasons_json: String::new(),
            reasons: Vec::new(),
            selector_version: String::new(),
            override_source: String::new(),
            supersedes_selection_id: None,
            created_at: String::new(),
        })
}

#[test]
fn a_mode_selection_round_trips_through_storage_with_its_reasons() {
    let selection = select_mode(
        &inputs(DecisionClass::Security),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    assert_eq!(selection.mode, CouncilMode::Full);

    let stored = round_trip(&selection);
    assert_eq!(
        stored.selection_id,
        selection.selection_id(PROJECT, Some(ROUND))
    );
    assert_eq!(stored.mode, "FULL");
    assert_eq!(stored.decision_class, "SECURITY");
    assert_eq!(stored.override_source, "NONE");
    assert_eq!(stored.round_id.as_deref(), Some(ROUND));
    assert_eq!(stored.selector_version, DEFAULT_SELECTOR_VERSION);
    assert_eq!(
        stored.reasons, selection.reasons,
        "the reason list survives the round trip"
    );
    assert!(stored.inputs_json.contains("\"affected_file_count\":12"));
    assert!(stored.inputs_json.contains("crates/council"));
}

#[test]
fn a_solo_selection_is_recorded_for_a_non_material_point_with_no_round() {
    // DEC-052: SOLO persists a record and opens no round, which is why round_id is nullable.
    //
    // The shared `inputs()` helper deliberately does not fit here: it carries a 12-file blast radius, which
    // crosses the shipped review threshold of 10, so the same ROUTINE class correctly computes REVIEW. That
    // escalation is the blast-radius input doing its job, and it has its own test below. SOLO is the routine
    // default for a decision point that stays under the review threshold, which is what this test pins.
    let routine_inputs = ModeInputs {
        decision_class: DecisionClass::Routine,
        blast_radius: BlastRadius::new(&["crates/council"], 3, false),
        prior_validation_failures: 0,
        open_disputes: 0,
    };
    let mut storage = database();
    let selection = select_mode(
        &routine_inputs,
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    assert_eq!(selection.mode, CouncilMode::Solo);
    assert!(!selection.requires_round());

    let clock = FixedClock::new(NOW);
    let request = selection.to_storage(&clock, PROJECT, None, NOW, None);
    assert_eq!(request.round_id, None);
    storage
        .insert_council_mode_selection(&request)
        .expect("insert selection");

    let records = storage.list_council_mode_selections(PROJECT).expect("list");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records.first().map(|record| record.mode.as_str()),
        Some("SOLO")
    );
    assert_eq!(
        records
            .first()
            .and_then(|record| record.round_id.as_deref()),
        None
    );
}

#[test]
fn a_user_override_is_recorded_as_user_and_may_lower_rigor() {
    let overridden = select_mode(
        &inputs(DecisionClass::DataLoss),
        Some(UserOverride::by_user(CouncilMode::Review)),
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    assert_eq!(overridden.computed_mode, CouncilMode::Full);
    assert_eq!(overridden.mode, CouncilMode::Review);
    assert!(overridden.was_overridden());

    let stored = round_trip(&overridden);
    assert_eq!(
        stored.mode, "REVIEW",
        "the lower mode is stored, not the computed one"
    );
    assert_eq!(stored.override_source, "USER");
    assert!(stored
        .reasons
        .iter()
        .any(|reason| reason.contains("user override lowered")));
}

#[test]
fn an_escalation_appends_a_superseding_record_rather_than_updating_one() {
    let mut storage = database();
    let clock = FixedClock::new(NOW);

    let first = select_mode(
        &inputs(DecisionClass::Architecture),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    let first_request =
        first.to_storage(&clock, PROJECT, Some(ROUND), "2026-10-04T10:00:00Z", None);
    let first_id = first_request.selection_id.clone();
    storage
        .insert_council_mode_selection(&first_request)
        .expect("insert first");

    // Escalation is upward only and only between rounds; it appends.
    let mut failed = inputs(DecisionClass::Architecture);
    failed.prior_validation_failures = 2;
    let escalated = select_mode(
        &failed,
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    assert_eq!(escalated.mode, CouncilMode::Full);
    let escalated_request = escalated.to_storage(
        &clock,
        PROJECT,
        Some(ROUND),
        "2026-10-04T10:05:00Z",
        Some(&first_id),
    );
    assert_ne!(
        escalated_request.selection_id, first_id,
        "an escalation is a new record"
    );
    storage
        .insert_council_mode_selection(&escalated_request)
        .expect("insert escalation");

    let records = storage.list_council_mode_selections(PROJECT).expect("list");
    assert_eq!(
        records.len(),
        2,
        "escalation appends; the earlier record is still there"
    );
    assert_eq!(
        records.first().map(|record| record.mode.as_str()),
        Some("REVIEW")
    );
    assert_eq!(
        records.get(1).map(|record| record.mode.as_str()),
        Some("FULL")
    );
    assert_eq!(
        records
            .get(1)
            .and_then(|record| record.supersedes_selection_id.as_deref()),
        Some(first_id.as_str())
    );
}

#[test]
fn a_rounds_roles_round_trip_and_keep_their_reasons() {
    let mut storage = database();
    let assignment = assign_round_roles(
        &["agent-hermes", "agent-kilo", "agent-opencode"],
        &[
            ("agent-hermes", "HERMES_AGENT"),
            ("agent-kilo", "KILO_CODE"),
            ("agent-opencode", "OPEN_CODE"),
        ],
        Some("agent-hermes"),
        0,
        7,
    );

    for record in assignment.records() {
        storage
            .insert_council_round_role(&NewCouncilRoundRole {
                round_id: ROUND.to_string(),
                agent_id: record.agent_id.clone(),
                role: record.role.as_str().to_string(),
                assigned_reason: record.assigned_reason.clone(),
                assigned_at: NOW.to_string(),
            })
            .expect("insert role");
    }

    let stored = storage.list_council_round_roles(ROUND).expect("list roles");
    assert_eq!(stored.len(), 3);
    let by_role = |role: &str| {
        stored
            .iter()
            .find(|row| row.role == role)
            .map(|row| row.agent_id.clone())
    };
    assert_eq!(by_role("PROPOSER").as_deref(), Some("agent-hermes"));
    assert!(by_role("SKEPTIC").is_some());
    assert!(by_role("VERIFIER").is_some());
    assert!(stored.iter().all(|row| !row.assigned_reason.is_empty()));
    assert!(stored
        .iter()
        .all(|row| RoleKind::parse(&row.role).is_some()));
}

#[test]
fn an_unavoidable_lineage_collision_is_recorded_in_the_stored_reason() {
    let mut storage = database();
    let assignment = assign_round_roles(
        &["agent-kilo", "agent-opencode"],
        &[("agent-kilo", "KILO_CODE"), ("agent-opencode", "OPEN_CODE")],
        Some("agent-kilo"),
        0,
        3,
    );
    assert!(assignment.lineage_unavoidable);

    for record in assignment.records() {
        storage
            .insert_council_round_role(&NewCouncilRoundRole {
                round_id: ROUND.to_string(),
                agent_id: record.agent_id.clone(),
                role: record.role.as_str().to_string(),
                assigned_reason: record.assigned_reason.clone(),
                assigned_at: NOW.to_string(),
            })
            .expect("insert role");
    }

    let stored = storage.list_council_round_roles(ROUND).expect("list roles");
    let skeptic = stored.iter().find(|row| row.role == "SKEPTIC");
    assert!(
        skeptic.is_some_and(|row| row.assigned_reason.contains(LINEAGE_UNAVOIDABLE)),
        "the stored reason must carry the token rather than hide the collision"
    );
}

#[test]
fn claim_grades_round_trip_with_the_basis_that_produced_them() {
    let mut storage = database();
    let position = Position::new("pos_a")
        .with_claim(Claim::load_bearing("claim_1").citing("evidence:ev_1"))
        .with_claim(Claim::load_bearing("claim_2").citing("check:cargo-test"))
        .with_claim(Claim::load_bearing("claim_3").citing("doc:missing"));

    let graded = grade_position(&position, &RepositoryFacts);
    assert_eq!(
        graded.grade(),
        Some(ClaimGrade::Assumption),
        "the weakest link governs"
    );

    for basis in graded.claim_grades() {
        storage
            .insert_council_claim_grade(&NewCouncilClaimGrade {
                claim_id: basis.claim_id.clone(),
                position_id: "pos_a".to_string(),
                round_id: ROUND.to_string(),
                grade: basis.grade.as_str().to_string(),
                load_bearing: true,
                basis: CouncilClaimGradeBasis {
                    claim_id: basis.claim_id.clone(),
                    reason: basis.reason.as_str().to_string(),
                    resolved: basis
                        .resolved_references
                        .iter()
                        .map(|reference| CouncilClaimReference {
                            reference: reference.clone(),
                            reason: "RESOLVED".to_string(),
                        })
                        .collect(),
                    unresolved: basis
                        .unresolved_references
                        .iter()
                        .map(|reference| CouncilClaimReference {
                            reference: reference.clone(),
                            reason: "REFERENCE_NOT_FOUND".to_string(),
                        })
                        .collect(),
                },
                computed_at: NOW.to_string(),
            })
            .expect("insert claim grade");
    }

    let stored = storage
        .list_council_claim_grades("pos_a")
        .expect("list grades");
    assert_eq!(stored.len(), 3, "every load-bearing claim is persisted");
    let grades: Vec<&str> = stored.iter().map(|row| row.grade.as_str()).collect();
    assert!(grades.contains(&"CITED"));
    assert!(grades.contains(&"VERIFIED"));
    assert!(grades.contains(&"ASSUMPTION"));

    let unresolved = stored
        .iter()
        .find(|row| row.claim_id == "claim_3")
        .map(|row| row.basis_json.clone())
        .unwrap_or_default();
    assert!(
        unresolved.contains("doc:missing"),
        "the basis records what did not resolve"
    );
    assert!(unresolved.contains("REFERENCE_NOT_FOUND"));

    // The stored grades reconstruct the same weakest-link verdict the grader computed.
    let reconstructed = stored
        .iter()
        .filter(|row| row.load_bearing)
        .filter_map(|row| ClaimGrade::parse(&row.grade))
        .fold(None, |weakest: Option<ClaimGrade>, grade| {
            Some(match weakest {
                Some(current) => current.weakest(grade),
                None => grade,
            })
        });
    assert_eq!(reconstructed, Some(ClaimGrade::Assumption));
    assert!(stored.iter().all(|row| row.load_bearing));
}

#[test]
fn a_position_with_no_load_bearing_claim_is_not_persisted_as_an_assumption() {
    // There is nothing to grade, so there is nothing to store. Persisting ASSUMPTION here would record a
    // claim the position never made.
    let position =
        Position::new("pos_empty").with_claim(Claim::supporting("c1").citing("evidence:ev_1"));
    let graded = grade_position(&position, &RepositoryFacts);
    assert_eq!(graded.grade(), None);
    assert_eq!(
        graded.reason(),
        mayasaba_council::GradeReason::NoLoadBearingClaims
    );
    assert!(graded.claim_grades().is_empty());

    let storage = database();
    assert!(storage
        .list_council_claim_grades("pos_empty")
        .expect("list grades")
        .is_empty());
}

#[test]
fn convergence_is_refused_from_the_persisted_grades_alone() {
    let position =
        Position::new("pos_a").with_claim(Claim::load_bearing("claim_1").citing("doc:missing"));
    let graded = grade_position(&position, &RepositoryFacts);

    let verdict = can_seal_converged(
        DecisionClass::Architecture,
        &["pos_a"],
        std::slice::from_ref(&graded),
        true,
        &ConvergencePrecondition::from_shipped_policy(),
    );
    match &verdict {
        ConvergenceVerdict::Refused(refusal) => {
            assert_eq!(refusal.position_ids(), &["pos_a".to_string()]);
        }
        ConvergenceVerdict::Permitted | ConvergenceVerdict::FixpointNotReached => {
            assert_eq!(
                verdict,
                ConvergenceVerdict::Permitted,
                "expected a quality refusal"
            )
        }
    }

    // A non-material decision point is not blocked by the shipped material-only precondition.
    let routine = can_seal_converged(
        DecisionClass::Routine,
        &["pos_a"],
        std::slice::from_ref(&graded),
        true,
        &ConvergencePrecondition::from_shipped_policy(),
    );
    assert_eq!(routine, ConvergenceVerdict::Permitted);
}

#[test]
fn kilo_and_opencode_are_one_corroboration_while_hermes_and_kilo_are_two() {
    let fork = count_lineage_groups(&["KILO_CODE", "OPEN_CODE"], DEFAULT_MINIMUM_LINEAGE_GROUPS);
    assert_eq!(fork.group_count, 1);
    assert!(fork.is_uncorroborated());

    let independent = count_lineage_groups(
        &["HERMES_AGENT", "KILO_CODE"],
        DEFAULT_MINIMUM_LINEAGE_GROUPS,
    );
    assert_eq!(independent.group_count, 2);
    assert!(independent.is_corroborated());
}

#[test]
fn a_synthesis_omission_is_detected_and_a_non_chair_reviewer_is_required() {
    let synthesis = SynthesisPosition::new("pos_syn", "agent-hermes", &["pos_a"]);
    let gap = check_synthesis_coverage(&synthesis, &["pos_a", "pos_b"]);
    assert_eq!(
        gap,
        Err(mayasaba_council::CoverageGap::OmittedPositions {
            position_ids: vec!["pos_b".to_string()],
        })
    );

    // The chair cannot review its own synthesis...
    let chair = mayasaba_council::check_reviewer_eligibility(
        &synthesis,
        "agent-hermes",
        &["agent-hermes", "agent-kilo"],
        "agent-hermes",
        None,
    );
    assert_eq!(
        chair,
        mayasaba_council::ReviewerEligibility::Refused(mayasaba_council::ReviewerRefusal::Chair)
    );

    // ...but a non-chair participant can.
    let reviewer = mayasaba_council::check_reviewer_eligibility(
        &synthesis,
        "agent-kilo",
        &["agent-hermes", "agent-kilo"],
        "agent-hermes",
        Some(RoleKind::Verifier),
    );
    assert!(reviewer.is_eligible());

    let complete = check_synthesis_coverage(&synthesis, &["pos_a"]);
    assert!(complete.is_ok());
}

#[test]
fn budget_exhaustion_yields_a_documented_outcome_and_paused_time_is_excluded() {
    let clock = FixedClock::new("2026-10-04T10:00:00Z");
    let ledger = BudgetLedger::new()
        .with_round_opened(&clock, "e1", Some(ROUND))
        .with_paused(&FixedClock::new("2026-10-04T10:10:00Z"), "e2", Some(ROUND))
        .with_resumed(&FixedClock::new("2026-10-04T10:40:00Z"), "e3", Some(ROUND))
        .with_spike("e4", Some(ROUND), "2026-10-04T10:41:00Z")
        .with_tokens_unavailable(
            "e5",
            Some(ROUND),
            "2026-10-04T10:42:00Z",
            "adapter reports no usage",
        );

    let caps = mayasaba_council::BudgetCaps {
        max_rounds: 1,
        max_wall_clock_seconds: 1_000,
        max_tokens: 100_000,
        max_spikes: 1,
    };
    let snapshot = compute_consumption(&ledger, &[], "2026-10-04T11:00:00Z");

    assert_eq!(snapshot.elapsed_seconds, Some(3_600));
    assert_eq!(snapshot.paused_seconds, 1_800);
    assert_eq!(
        snapshot.active_seconds,
        Some(1_800),
        "paused time is excluded from wall-clock"
    );
    assert_eq!(
        snapshot.tokens, None,
        "an unreported token count is never invented"
    );
    assert!(snapshot.tokens_unavailable);

    let spent = exhaustion(&snapshot, &caps);
    assert!(spent.exceeded.contains(&BudgetDimension::Rounds));
    assert!(spent.exceeded.contains(&BudgetDimension::Spikes));
    assert!(spent.exceeded.contains(&BudgetDimension::WallClock));
    assert_eq!(spent.unmeasurable, vec![BudgetDimension::Tokens]);
    assert_eq!(
        spent.outcome(),
        Some(mayasaba_council::BudgetExhaustionOutcome::Escalated),
        "a spent budget always yields a documented outcome"
    );
}

#[test]
fn the_budget_ledger_round_trips_and_an_unavailable_amount_stays_null() {
    let mut storage = database();
    let clock = FixedClock::new("2026-10-04T10:00:00Z");
    let ledger = BudgetLedger::new()
        .with_round_opened(&clock, "e1", Some(ROUND))
        .with_tokens_unavailable(
            "e2",
            Some(ROUND),
            "2026-10-04T10:01:00Z",
            "adapter reports no usage",
        );

    for entry in ledger.entries() {
        storage
            .insert_council_budget_entry(&mayasaba_storage::NewCouncilBudgetLedgerEntry {
                entry_id: entry.entry_id.clone(),
                council_session_id: Some(COUNCIL.to_string()),
                round_id: entry.round_id.clone(),
                kind: entry.kind.as_str().to_string(),
                amount: entry.amount.map(|amount| amount as i64),
                availability: entry.availability.as_str().to_string(),
                detail: entry.detail.clone(),
                recorded_at: entry.recorded_at.clone(),
            })
            .expect("insert ledger entry");
    }

    let stored = storage
        .list_council_budget_ledger(COUNCIL)
        .expect("list ledger");
    assert_eq!(stored.len(), 2);
    let unavailable = stored.iter().find(|row| row.entry_id == "e2");
    assert_eq!(
        unavailable.and_then(|row| row.amount),
        None,
        "no number is invented for UNAVAILABLE"
    );
    assert_eq!(
        unavailable.map(|row| row.availability.as_str()),
        Some("UNAVAILABLE")
    );
    assert!(unavailable
        .and_then(|row| row.detail.as_deref())
        .is_some_and(|detail| detail.contains("no usage")));

    // The database refuses an UNAVAILABLE row that carries a number. This is what makes the rule structural
    // rather than merely observed.
    let refused =
        storage.insert_council_budget_entry(&mayasaba_storage::NewCouncilBudgetLedgerEntry {
            entry_id: "e3".to_string(),
            council_session_id: Some(COUNCIL.to_string()),
            round_id: Some(ROUND.to_string()),
            kind: "TOKENS_REPORTED".to_string(),
            amount: Some(1234),
            availability: "UNAVAILABLE".to_string(),
            detail: None,
            recorded_at: NOW.to_string(),
        });
    assert!(
        refused.is_err(),
        "an estimated token count must not reach the ledger"
    );

    let by_round = storage
        .list_council_budget_ledger_for_round(ROUND)
        .expect("list by round");
    assert_eq!(by_round.len(), 2);
}

#[test]
fn a_held_outcome_requires_validation_evidence_and_the_database_agrees() {
    let clock = FixedClock::new(NOW);
    let refused = OutcomeRecordBuilder::new(
        "out_1",
        "dec_1",
        CouncilMode::Review,
        DecisionClass::Architecture,
        OutcomeStatus::Held,
        OutcomeSource::ValidationResult,
    )
    .build(&clock);
    assert!(
        refused.is_err(),
        "HELD without validation evidence is refused before storage"
    );

    let held = OutcomeRecordBuilder::new(
        "out_1",
        "dec_1",
        CouncilMode::Review,
        DecisionClass::Architecture,
        OutcomeStatus::Held,
        OutcomeSource::ValidationResult,
    )
    .validated_by("ev_1")
    .in_session(COUNCIL)
    .in_round(ROUND)
    .with_link(OutcomeAgentLink::new("agent-hermes", "pos_a", "AGREED"))
    .with_link(OutcomeAgentLink::without_position(
        "agent-kilo",
        "DISSENTED",
    ))
    .build(&clock)
    .expect("build held outcome");
    assert!(held.asserts_held());
    assert!(held.informational_only());

    let mut storage = database();
    storage
        .insert_council_decision_outcome(&held.to_storage())
        .expect("insert outcome");

    let stored = storage
        .list_council_decision_outcomes("dec_1")
        .expect("list outcomes");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.first().map(|row| row.status.as_str()), Some("HELD"));
    assert_eq!(
        stored
            .first()
            .and_then(|row| row.validation_evidence_id.as_deref()),
        Some("ev_1")
    );

    let links = storage
        .list_council_outcome_agent_links("out_1")
        .expect("list links");
    assert_eq!(links.len(), 2);
    assert_eq!(
        links.first().map(|row| row.agent_id.as_str()),
        Some("agent-hermes")
    );

    // The table's own CHECK constraint refuses a HELD record with no evidence, so the rule holds even for a
    // caller that bypasses the builder.
    let bypass = storage.insert_council_decision_outcome(&NewCouncilDecisionOutcome {
        outcome_record_id: "out_bypass".to_string(),
        decision_id: "dec_1".to_string(),
        council_session_id: None,
        round_id: None,
        mode: "REVIEW".to_string(),
        decision_class: "ARCHITECTURE".to_string(),
        status: "HELD".to_string(),
        validation_evidence_id: None,
        source: "VALIDATION_RESULT".to_string(),
        supersedes_outcome_id: None,
        recorded_at: NOW.to_string(),
        agent_links: Vec::new(),
    });
    assert!(
        bypass.is_err(),
        "the database enforces the same rule the domain does"
    );
}

#[test]
fn supersession_appends_and_history_is_preserved() {
    let clock = FixedClock::new(NOW);
    let mut storage = database();

    let held = OutcomeRecordBuilder::new(
        "out_1",
        "dec_1",
        CouncilMode::Review,
        DecisionClass::Architecture,
        OutcomeStatus::Held,
        OutcomeSource::ValidationResult,
    )
    .validated_by("ev_1")
    .build(&clock)
    .expect("build held");

    let reversed = OutcomeRecordBuilder::new(
        "out_2",
        "dec_1",
        CouncilMode::Review,
        DecisionClass::Architecture,
        OutcomeStatus::Reversed,
        OutcomeSource::ReopenDecision,
    )
    .superseding("out_1")
    .build(&clock)
    .expect("build reversal");

    storage
        .insert_council_decision_outcome(&held.to_storage())
        .expect("insert held");
    storage
        .insert_council_decision_outcome(&reversed.to_storage())
        .expect("insert reversal");

    let stored = storage
        .list_council_decision_outcomes("dec_1")
        .expect("list");
    assert_eq!(
        stored.len(),
        2,
        "a reversal appends; the HELD record is still there"
    );
    assert_eq!(stored.first().map(|row| row.status.as_str()), Some("HELD"));
    assert_eq!(
        stored.get(1).map(|row| row.status.as_str()),
        Some("REVERSED")
    );
    assert_eq!(
        stored
            .get(1)
            .and_then(|row| row.supersedes_outcome_id.as_deref()),
        Some("out_1")
    );

    // The same history through the append-only domain store.
    let store = OutcomeStore::new()
        .appended(held.clone())
        .appended(reversed.clone());
    assert_eq!(store.len(), 2);
    assert!(
        !store.is_held("dec_1"),
        "the newest record decides what is reported"
    );
    assert_eq!(
        store.records().first().map(|record| record.status),
        Some(OutcomeStatus::Held)
    );
    assert_eq!(store.for_decision("dec_1").len(), 2);
}

#[test]
fn mode_selection_is_identical_with_and_without_outcome_data_present() {
    // DEC-052 limitation 2: outcome tracking is informational only and must not influence mode selection.
    // The strongest available evidence for that is that the same inputs produce a byte-identical record
    // whether or not outcomes exist, which cannot be true of a function that reads them.
    let mut quiet_storage = database();
    let mut busy_storage = database();
    let clock = FixedClock::new(NOW);

    let quiet = select_mode(
        &inputs(DecisionClass::StackTechnology),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    let quiet_record = quiet.to_storage(&clock, PROJECT, Some(ROUND), NOW, None);
    quiet_storage
        .insert_council_mode_selection(&quiet_record)
        .expect("insert quiet");

    // Populate outcomes for the same project, including a REVERSED one, which is the state most likely to
    // tempt a naive implementation into raising rigor.
    for (id, status, validation) in [
        ("out_a", OutcomeStatus::Held, Some("ev_1")),
        ("out_b", OutcomeStatus::Amended, None),
        ("out_c", OutcomeStatus::Reversed, None),
        ("out_d", OutcomeStatus::Unresolved, None),
    ] {
        let mut builder = OutcomeRecordBuilder::new(
            id,
            "dec_1",
            CouncilMode::Solo,
            DecisionClass::StackTechnology,
            status,
            OutcomeSource::ValidationResult,
        );
        if let Some(evidence) = validation {
            builder = builder.validated_by(evidence);
        }
        if status.requires_superseded_outcome() {
            builder = builder.superseding("out_a");
        }
        let record = builder.build(&clock).expect("build outcome");
        busy_storage
            .insert_council_decision_outcome(&record.to_storage())
            .expect("insert outcome");
    }
    assert_eq!(
        busy_storage
            .list_council_decision_outcomes("dec_1")
            .expect("list")
            .len(),
        4
    );

    let busy = select_mode(
        &inputs(DecisionClass::StackTechnology),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    let busy_record = busy.to_storage(&clock, PROJECT, Some(ROUND), NOW, None);
    busy_storage
        .insert_council_mode_selection(&busy_record)
        .expect("insert busy");

    // The selection is the same value, the same persistence request and the same derived id.
    assert_eq!(quiet, busy);
    assert_eq!(quiet_record, busy_record);
    assert_eq!(quiet_record.selection_id, busy_record.selection_id);
    assert_eq!(quiet_record.reasons, busy_record.reasons);

    // And the stored records agree, which is what a reader would compare.
    let quiet_stored = quiet_storage
        .list_council_mode_selections(PROJECT)
        .expect("list quiet");
    let busy_stored = busy_storage
        .list_council_mode_selections(PROJECT)
        .expect("list busy");
    assert_eq!(quiet_stored.len(), 1);
    assert_eq!(busy_stored.len(), 1);
    assert_eq!(quiet_stored.first(), busy_stored.first());
}

#[test]
fn outcome_data_cannot_reach_the_selector_because_a_selection_has_no_outcome_input() {
    // The structural half of the same rule: `select_mode` takes inputs, an override and thresholds. There is
    // no parameter an outcome could travel through, so the property above is not a coincidence of an
    // implementation that happens to ignore it.
    let selection = select_mode(
        &inputs(DecisionClass::Architecture),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    );
    let rendered = format!("{selection:?}");
    assert!(!rendered.contains("outcome"));
    assert!(!rendered.contains("HELD"));
    assert!(!rendered.contains("status"));

    // A store full of outcomes is inert with respect to selection.
    let clock = FixedClock::new(NOW);
    let held: CouncilDecisionOutcome = OutcomeRecordBuilder::new(
        "out_1",
        "dec_1",
        CouncilMode::Review,
        DecisionClass::Architecture,
        OutcomeStatus::Held,
        OutcomeSource::ValidationResult,
    )
    .validated_by("ev_1")
    .build(&clock)
    .expect("build");
    let store = OutcomeStore::new().appended(held);
    assert!(store.is_held("dec_1"));
    assert_eq!(
        select_mode(
            &inputs(DecisionClass::Architecture),
            None,
            &shipped_thresholds(),
            DEFAULT_SELECTOR_VERSION
        ),
        selection
    );
}

#[test]
fn mode_selection_determinism_survives_persistence() {
    // Two identical selections in two databases produce the same stored record, which is what "deterministic"
    // has to mean once a record is durable.
    let first = round_trip(&select_mode(
        &inputs(DecisionClass::Irreversible),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    ));
    let second = round_trip(&select_mode(
        &inputs(DecisionClass::Irreversible),
        None,
        &shipped_thresholds(),
        DEFAULT_SELECTOR_VERSION,
    ));
    assert_eq!(first, second);
}
