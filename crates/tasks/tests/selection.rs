//! Characterization tests for task selection and the lease/attempt surface.
//!
//! These pin behaviour that already exists. They were written before any of it was changed, and they must
//! pass on the unchanged code, so that a later change to selection has a regression net rather than an
//! assertion of intent. Nothing here asserts what selection *should* do; every assertion states what it does.
//!
//! Two fixture facts are worth stating, because they explain the shape of the setup rather than a shortcut:
//!
//! * `tasks`, `task_scopes` and `admissions` rows are inserted with raw SQL. No storage API writes `tasks`:
//!   the task domain has no production writer yet, so the table is populated the way the existing
//!   orchestration tests populate it. `Storage::insert_admission` *does* exist and validates its own shape,
//!   but the lease gate reads only `kind`, `verdict`, `epoch` and `context_digest` from it, so the fixture
//!   inserts the row it needs and leaves that API's validation to the tests that own it.
//! * `lease_task` is gated on a current `ADMITTED` workspace admission, a `task_scopes` row that the lease
//!   request must equal exactly, and a live `context_snapshots` row whose digest matches the lease. The
//!   fixture builds all four, because a lease admitted without them would not be the thing under test.

use mayasaba_storage::{NewProject, NewTaskAttempt, NewTaskLease, Storage};
use mayasaba_tasks::{
    advance_attempt, authorize_material_action, lease_task, release_lease, renew_lease,
    reserve_child_slot, revoke_lease, select_schedulable_tasks, start_attempt,
};

const PROJECT: &str = "prj_sel";
const WORKSPACE: &str = "ws_sel";
const AGENT: &str = "agent_sel";
const SESSION: &str = "sess_sel";
const CONTEXT: &str = "ctx_sel";
const ROOT: &str = r"C:\work\sel";
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCOPE_POLICY: &str = "PROJECT_WRITE";

/// A project with an enabled agent, one workspace, one healthy session and one live context snapshot.
///
/// The project's `current_epoch` is 0 after `create_project`, so every task fixture below is created at
/// epoch 0 unless a test is deliberately testing the epoch gate.
fn project_storage() -> Storage {
    let mut storage = Storage::open_in_memory().expect("open in-memory store");
    storage
        .create_project(&NewProject {
            project_id: PROJECT.into(),
            local_path: ROOT.into(),
            brief_id: "brief_sel".into(),
            brief_body: "selection".into(),
            brief_source: "TEST".into(),
            event_id: "evt_sel".into(),
            created_at: "1".into(),
        })
        .expect("project");
    storage
        .conn()
        .execute(
            "INSERT INTO agents (agent_id,agent_type,executable,created_at,updated_at)
             VALUES (?1,'HERMES_AGENT','hermes','1','1')",
            [AGENT],
        )
        .expect("agent");
    storage
        .conn()
        .execute(
            "INSERT INTO workspaces (workspace_id,project_id,kind,root_path,status,created_at)
             VALUES (?1,?2,'AGENT',?3,'ACTIVE','1')",
            (WORKSPACE, PROJECT, ROOT),
        )
        .expect("workspace");
    storage
        .conn()
        .execute(
            "INSERT INTO agent_sessions
             (session_id,project_id,agent_id,state,health_state,workspace_id,current_epoch,started_at)
             VALUES (?1,?2,?3,'READY','HEALTHY',?4,0,'1')",
            (SESSION, PROJECT, AGENT, WORKSPACE),
        )
        .expect("agent session");
    storage
        .conn()
        .execute(
            "INSERT INTO context_snapshots
             (context_snapshot_id,project_id,epoch,scope,state_digest,pack_json,created_at)
             VALUES (?1,?2,0,'PROJECT',?3,'{}','1')",
            (CONTEXT, PROJECT, DIGEST),
        )
        .expect("context snapshot");
    storage
}

/// Insert a task row directly, with every gate-relevant column under the test's control.
#[allow(clippy::too_many_arguments)]
fn add_task(
    storage: &Storage,
    task_id: &str,
    priority: i64,
    status: &str,
    epoch: i64,
    workspace: Option<&str>,
    created_at: &str,
    updated_at: &str,
) {
    storage
        .conn()
        .execute(
            "INSERT INTO tasks
             (task_id,project_id,objective,status,priority,risk,workspace_id,current_epoch,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,'LOW',?6,?7,?8,?9)",
            (
                task_id,
                PROJECT,
                format!("objective for {task_id}"),
                status,
                priority,
                workspace,
                epoch,
                created_at,
                updated_at,
            ),
        )
        .expect("insert task");
}

/// The common case: a READY task at the project's epoch, in the project workspace, whose creation and
/// update timestamps are equal.
fn add_ready(storage: &Storage, task_id: &str, priority: i64, at: &str) {
    add_task(
        storage,
        task_id,
        priority,
        "READY",
        0,
        Some(WORKSPACE),
        at,
        at,
    );
}

fn depend(storage: &Storage, task_id: &str, on: &str) {
    storage
        .conn()
        .execute(
            "INSERT INTO task_dependencies (task_id,depends_on_task_id) VALUES (?1,?2)",
            (task_id, on),
        )
        .expect("insert dependency");
}

fn complete(storage: &Storage, task_id: &str) {
    storage
        .conn()
        .execute(
            "UPDATE tasks SET status='COMPLETED' WHERE task_id=?1",
            [task_id],
        )
        .expect("complete task");
}

fn add_lease(storage: &Storage, lease_id: &str, task_id: &str, status: &str, version: i64) {
    storage
        .conn()
        .execute(
            "INSERT INTO task_leases
             (lease_id,task_id,project_id,agent_id,session_id,workspace_id,lease_version,project_epoch,
              context_snapshot_id,state_digest,allowed_paths_json,required_capabilities_json,policy_scope,
              issued_at,heartbeat_at,expires_at,status)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?9,'[]','[]',?10,'1','1','9999',?11)",
            (
                lease_id,
                task_id,
                PROJECT,
                AGENT,
                SESSION,
                WORKSPACE,
                version,
                CONTEXT,
                DIGEST,
                SCOPE_POLICY,
                status,
            ),
        )
        .expect("insert lease");
}

/// Give a task the durable scope and the current ADMITTED workspace admission that lease admission requires.
fn make_leaseable(storage: &Storage, task_id: &str) {
    storage
        .conn()
        .execute(
            "INSERT INTO task_scopes
             (task_id,allowed_paths_json,required_capabilities_json,validation_requirements_json,
              policy_scope,max_attempts,max_parallel_children,created_at,updated_at)
             VALUES (?1,'[]','[]','[]',?2,3,2,'1','1')",
            (task_id, SCOPE_POLICY),
        )
        .expect("task scope");
    storage
        .conn()
        .execute(
            "INSERT INTO admissions
             (admission_id,project_id,task_id,workspace_id,agent_id,session_id,kind,epoch,context_digest,
              changed_paths_json,checks_json,verdict,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,'WORKSPACE_ADMISSION',0,?7,'[]',
                     '[{\"check_id\":\"REPOSITORY_IDENTITY\",\"status\":\"PASS\"}]','ADMITTED','1')",
            (
                format!("adm_{task_id}"),
                PROJECT,
                task_id,
                WORKSPACE,
                AGENT,
                SESSION,
                DIGEST,
            ),
        )
        .expect("admission");
}

fn selected(storage: &Storage, limit: usize) -> Vec<String> {
    select_schedulable_tasks(storage, PROJECT, limit)
        .expect("select")
        .into_iter()
        .map(|task| task.task_id)
        .collect()
}

fn task_status(storage: &Storage, task_id: &str) -> String {
    storage
        .conn()
        .query_row(
            "SELECT status FROM tasks WHERE task_id=?1",
            [task_id],
            |row| row.get(0),
        )
        .expect("task status")
}

// ---------------------------------------------------------------------------------------------------
// Selection: determinism
// ---------------------------------------------------------------------------------------------------

#[test]
fn selection_is_deterministic_for_equal_inputs() {
    let build = || {
        let storage = project_storage();
        add_ready(&storage, "task_a", 5, "2");
        add_ready(&storage, "task_b", 5, "1");
        add_ready(&storage, "task_c", 9, "5");
        storage
    };
    let first = build();
    let second = build();

    // Equal inputs in two separate databases produce the same order...
    assert_eq!(selected(&first, 10), selected(&second, 10));
    // ...and repeated calls against one database do not drift.
    assert_eq!(selected(&first, 10), selected(&first, 10));
    assert_eq!(selected(&first, 10), vec!["task_c", "task_b", "task_a"]);
}

#[test]
fn ordering_is_priority_then_update_then_creation_then_task_id() {
    let storage = project_storage();
    // No task here depends on another, so each one's own priority is the only priority in play. This is
    // deliberate: it keeps the expected order a statement about the existing tie-break chain alone.
    add_task(
        &storage,
        "task_p5_up2",
        5,
        "READY",
        0,
        Some(WORKSPACE),
        "0",
        "2",
    );
    add_task(
        &storage,
        "task_p5_up1_c1",
        5,
        "READY",
        0,
        Some(WORKSPACE),
        "1",
        "1",
    );
    add_task(
        &storage,
        "task_p5_up1_c0",
        5,
        "READY",
        0,
        Some(WORKSPACE),
        "0",
        "1",
    );
    add_task(
        &storage,
        "task_p9",
        9,
        "READY",
        0,
        Some(WORKSPACE),
        "9",
        "9",
    );
    add_task(
        &storage,
        "task_p1",
        1,
        "READY",
        0,
        Some(WORKSPACE),
        "0",
        "0",
    );

    assert_eq!(
        selected(&storage, 10),
        vec![
            // priority DESC
            "task_p9",
            // then updated_at ASC; the two at "1" are separated by created_at ASC
            "task_p5_up1_c0",
            "task_p5_up1_c1",
            // then the one updated later
            "task_p5_up2",
            // then the lowest priority
            "task_p1",
        ]
    );
}

#[test]
fn tasks_at_equal_priority_and_equal_timestamps_are_separated_by_task_id() {
    let storage = project_storage();
    add_ready(&storage, "task_zzz", 3, "7");
    add_ready(&storage, "task_aaa", 3, "7");
    add_ready(&storage, "task_mmm", 3, "7");

    assert_eq!(
        selected(&storage, 10),
        vec!["task_aaa", "task_mmm", "task_zzz"]
    );
}

// ---------------------------------------------------------------------------------------------------
// Selection: dependency readiness
// ---------------------------------------------------------------------------------------------------

#[test]
fn a_task_whose_dependency_has_not_completed_is_not_selectable() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_ready(&storage, "task_dependent", 99, "1");
    depend(&storage, "task_dependent", "task_prereq");

    // The dependent outranks the prerequisite by its own priority, and is still excluded: readiness is a
    // gate, not an ordering input.
    assert_eq!(selected(&storage, 10), vec!["task_prereq"]);

    complete(&storage, "task_prereq");
    assert_eq!(
        selected(&storage, 10),
        vec!["task_dependent"],
        "a completed dependency releases the task that was waiting on it"
    );
}

#[test]
fn every_dependency_of_a_task_must_complete_before_it_is_selectable() {
    let storage = project_storage();
    add_ready(&storage, "task_first", 1, "1");
    add_ready(&storage, "task_second", 1, "1");
    add_ready(&storage, "task_third", 1, "1");
    depend(&storage, "task_third", "task_first");
    depend(&storage, "task_third", "task_second");

    complete(&storage, "task_first");
    assert!(
        !selected(&storage, 10).contains(&"task_third".to_string()),
        "one satisfied dependency out of two is not readiness"
    );

    complete(&storage, "task_second");
    assert!(selected(&storage, 10).contains(&"task_third".to_string()));
}

#[test]
fn a_dependency_chain_releases_one_link_at_a_time() {
    let storage = project_storage();
    add_ready(&storage, "task_c", 1, "1");
    add_ready(&storage, "task_b", 1, "1");
    add_ready(&storage, "task_a", 1, "1");
    depend(&storage, "task_b", "task_c");
    depend(&storage, "task_a", "task_b");

    assert_eq!(selected(&storage, 10), vec!["task_c"]);
    complete(&storage, "task_c");
    assert_eq!(selected(&storage, 10), vec!["task_b"]);
    complete(&storage, "task_b");
    assert_eq!(selected(&storage, 10), vec!["task_a"]);
}

#[test]
fn a_completed_dependency_does_not_itself_appear_as_selectable() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_ready(&storage, "task_dependent", 2, "1");
    depend(&storage, "task_dependent", "task_prereq");
    complete(&storage, "task_prereq");

    let rows = selected(&storage, 10);
    assert_eq!(rows, vec!["task_dependent"]);
}

// ---------------------------------------------------------------------------------------------------
// Selection: the other gates
// ---------------------------------------------------------------------------------------------------

#[test]
fn only_ready_tasks_are_selectable() {
    let storage = project_storage();
    for status in [
        "PLANNED",
        "LEASE_REQUESTED",
        "LEASED",
        "ACCEPTED",
        "IN_PROGRESS",
        "REVIEW_PENDING",
        "VALIDATION_PENDING",
        "PASSED",
        "COMPLETED",
        "BLOCKED",
        "FAILED",
        "RETRY_PENDING",
        "REPAIR_PENDING",
        "LEASE_EXPIRED",
        "RECOVERY_PENDING",
        "REASSIGNED",
        "INVALIDATED",
    ] {
        add_task(
            &storage,
            &format!("task_{status}"),
            1,
            status,
            0,
            Some(WORKSPACE),
            "1",
            "1",
        );
    }
    add_ready(&storage, "task_ready", 1, "1");

    assert_eq!(selected(&storage, 10), vec!["task_ready"]);
}

#[test]
fn a_task_at_a_stale_epoch_is_not_selectable() {
    let storage = project_storage();
    add_task(
        &storage,
        "task_current",
        1,
        "READY",
        0,
        Some(WORKSPACE),
        "1",
        "1",
    );
    add_task(
        &storage,
        "task_stale",
        99,
        "READY",
        1,
        Some(WORKSPACE),
        "1",
        "1",
    );

    assert_eq!(
        selected(&storage, 10),
        vec!["task_current"],
        "a task whose epoch no longer matches the project is withheld regardless of priority"
    );
}

#[test]
fn a_task_with_no_workspace_is_not_selectable() {
    let storage = project_storage();
    add_ready(&storage, "task_scoped", 1, "1");
    add_task(&storage, "task_unscoped", 99, "READY", 0, None, "1", "1");

    assert_eq!(selected(&storage, 10), vec!["task_scoped"]);
}

#[test]
fn a_task_with_a_live_lease_is_not_selectable_and_a_released_one_is() {
    let storage = project_storage();
    add_ready(&storage, "task_active", 9, "1");
    add_ready(&storage, "task_renewing", 8, "1");
    add_ready(&storage, "task_released", 7, "1");
    add_ready(&storage, "task_revoked", 6, "1");
    add_lease(&storage, "lease_active", "task_active", "ACTIVE", 1);
    add_lease(&storage, "lease_renewing", "task_renewing", "RENEWING", 1);
    add_lease(&storage, "lease_released", "task_released", "RELEASED", 1);
    add_lease(&storage, "lease_revoked", "task_revoked", "REVOKED", 1);

    assert_eq!(
        selected(&storage, 10),
        vec!["task_released", "task_revoked"],
        "only ACTIVE and RENEWING leases withhold a task; a finished lease does not"
    );
}

#[test]
fn the_limit_is_clamped_and_never_returns_more_than_the_candidates() {
    let storage = project_storage();
    add_ready(&storage, "task_1", 3, "1");
    add_ready(&storage, "task_2", 2, "1");
    add_ready(&storage, "task_3", 1, "1");

    // A zero limit is raised to one rather than returning nothing.
    assert_eq!(selected(&storage, 0), vec!["task_1"]);
    assert_eq!(selected(&storage, 1), vec!["task_1"]);
    assert_eq!(selected(&storage, 2), vec!["task_1", "task_2"]);
    // A limit larger than the candidate set is not an error and does not pad the result.
    assert_eq!(selected(&storage, 256), vec!["task_1", "task_2", "task_3"]);
    assert_eq!(selected(&storage, 4096), vec!["task_1", "task_2", "task_3"]);
}

#[test]
fn selection_performs_no_mutation() {
    let storage = project_storage();
    add_ready(&storage, "task_a", 2, "1");
    add_ready(&storage, "task_b", 1, "1");
    depend(&storage, "task_b", "task_a");
    add_lease(&storage, "lease_a", "task_a", "ACTIVE", 3);

    let before = task_status(&storage, "task_a");
    let selected_before = selected(&storage, 10);
    let selected_after = selected(&storage, 10);

    assert_eq!(selected_before, selected_after);
    assert_eq!(task_status(&storage, "task_a"), before);
    assert_eq!(
        storage
            .conn()
            .query_row("SELECT COUNT(*) FROM task_leases", [], |row| row
                .get::<_, i64>(0))
            .expect("lease count"),
        1,
        "selection does not create, renew or expire a lease"
    );
}

#[test]
fn selection_is_scoped_to_one_project() {
    let storage = project_storage();
    add_ready(&storage, "task_mine", 1, "1");

    let other = select_schedulable_tasks(&storage, "prj_someone_else", 10).expect("select other");
    assert!(other.is_empty());
    assert_eq!(selected(&storage, 10), vec!["task_mine"]);
}

// ---------------------------------------------------------------------------------------------------
// Lease admission and attempt fencing
// ---------------------------------------------------------------------------------------------------

fn lease_request(task_id: &str) -> NewTaskLease {
    NewTaskLease {
        lease_id: format!("lease_{task_id}"),
        task_id: task_id.into(),
        project_id: PROJECT.into(),
        agent_id: AGENT.into(),
        session_id: SESSION.into(),
        workspace_id: WORKSPACE.into(),
        project_epoch: 0,
        context_snapshot_id: CONTEXT.into(),
        state_digest: DIGEST.into(),
        allowed_paths_json: "[]".into(),
        required_capabilities_json: "[]".into(),
        policy_scope: SCOPE_POLICY.into(),
        issued_at: "1".into(),
        heartbeat_at: "1".into(),
        expires_at: "9999".into(),
    }
}

#[test]
fn leasing_a_ready_task_issues_the_first_fence_version() {
    let mut storage = project_storage();
    add_ready(&storage, "task_lease", 1, "1");
    make_leaseable(&storage, "task_lease");

    let lease = lease_task(&mut storage, &lease_request("task_lease")).expect("lease admitted");
    assert_eq!(lease.lease_version, 1, "the first lease is version 1");
    assert_eq!(lease.status, "ACTIVE");
    assert_eq!(lease.task_id, "task_lease");
}

#[test]
fn a_task_cannot_be_leased_without_a_current_admitted_workspace_admission() {
    let mut storage = project_storage();
    add_ready(&storage, "task_no_admission", 1, "1");

    let refused = lease_task(&mut storage, &lease_request("task_no_admission"));
    assert!(
        refused.is_err(),
        "the workspace admission gate is not optional: a lease was issued without one"
    );
}

#[test]
fn a_second_live_lease_for_the_same_task_is_refused() {
    let mut storage = project_storage();
    add_ready(&storage, "task_twice", 1, "1");
    make_leaseable(&storage, "task_twice");

    lease_task(&mut storage, &lease_request("task_twice")).expect("first lease");
    let mut second = lease_request("task_twice");
    second.lease_id = "lease_twice_2".into();
    assert!(
        lease_task(&mut storage, &second).is_err(),
        "one task may not carry two live leases"
    );
}

#[test]
fn a_lease_at_the_wrong_epoch_is_refused() {
    let mut storage = project_storage();
    add_ready(&storage, "task_epoch", 1, "1");
    make_leaseable(&storage, "task_epoch");

    let mut stale = lease_request("task_epoch");
    stale.project_epoch = 7;
    assert!(lease_task(&mut storage, &stale).is_err());
}

#[test]
fn renewal_and_release_require_the_current_fence_version() {
    let mut storage = project_storage();
    add_ready(&storage, "task_fence", 1, "1");
    make_leaseable(&storage, "task_fence");
    let lease = lease_task(&mut storage, &lease_request("task_fence")).expect("lease");

    assert!(
        renew_lease(
            &mut storage,
            &lease.lease_id,
            lease.lease_version + 1,
            "2",
            "9999"
        )
        .is_err(),
        "a superseded fence version cannot renew"
    );
    assert!(
        release_lease(&mut storage, &lease.lease_id, lease.lease_version + 1, "2").is_err(),
        "a superseded fence version cannot release"
    );
    assert!(
        revoke_lease(&mut storage, &lease.lease_id, lease.lease_version + 1, "2").is_err(),
        "a superseded fence version cannot revoke"
    );

    let renewed = renew_lease(
        &mut storage,
        &lease.lease_id,
        lease.lease_version,
        "2",
        "9999",
    )
    .expect("renew");
    assert_eq!(renewed.lease_version, lease.lease_version);
    release_lease(&mut storage, &lease.lease_id, lease.lease_version, "3").expect("release");
}

#[test]
fn an_attempt_is_created_against_a_live_lease_and_its_fence_is_checked() {
    let mut storage = project_storage();
    add_ready(&storage, "task_attempt", 1, "1");
    make_leaseable(&storage, "task_attempt");
    let lease = lease_task(&mut storage, &lease_request("task_attempt")).expect("lease");

    let attempt = NewTaskAttempt {
        attempt_id: "att_1".into(),
        task_id: "task_attempt".into(),
        project_id: PROJECT.into(),
        attempt_no: 1,
        lease_id: lease.lease_id.clone(),
        agent_id: AGENT.into(),
        session_id: SESSION.into(),
        workspace_id: WORKSPACE.into(),
        fence_token: lease.lease_version,
        project_epoch: 0,
        context_snapshot_id: CONTEXT.into(),
        state: "CREATED".into(),
        checkpoint_id: None,
        failure_id: None,
        started_at: None,
        heartbeat_at: None,
        ended_at: None,
        created_at: "1".into(),
    };
    let authorization = start_attempt(&storage, &attempt).expect("attempt created");
    assert_eq!(authorization.attempt_no, 1);
    assert_eq!(authorization.fence_token, lease.lease_version);

    // An attempt that has not started authorizes nothing: the fence check only considers STARTED, RUNNING
    // and CHECKPOINTED attempts, so creation alone is not authority.
    assert!(
        authorize_material_action(&storage, "att_1", lease.lease_version).is_err(),
        "a CREATED attempt must not authorize a material action"
    );

    advance_attempt(&storage, "att_1", "CREATED", "STARTED", None).expect("started");
    // The fence that authorized creation still authorizes a material action...
    authorize_material_action(&storage, "att_1", lease.lease_version).expect("authorized");
    // ...and a superseded one does not.
    assert!(authorize_material_action(&storage, "att_1", lease.lease_version + 1).is_err());
}

#[test]
fn attempt_state_transitions_are_compare_and_swap() {
    let mut storage = project_storage();
    add_ready(&storage, "task_cas", 1, "1");
    make_leaseable(&storage, "task_cas");
    let lease = lease_task(&mut storage, &lease_request("task_cas")).expect("lease");
    start_attempt(
        &storage,
        &NewTaskAttempt {
            attempt_id: "att_cas".into(),
            task_id: "task_cas".into(),
            project_id: PROJECT.into(),
            attempt_no: 1,
            lease_id: lease.lease_id.clone(),
            agent_id: AGENT.into(),
            session_id: SESSION.into(),
            workspace_id: WORKSPACE.into(),
            fence_token: lease.lease_version,
            project_epoch: 0,
            context_snapshot_id: CONTEXT.into(),
            state: "CREATED".into(),
            checkpoint_id: None,
            failure_id: None,
            started_at: None,
            heartbeat_at: None,
            ended_at: None,
            created_at: "1".into(),
        },
    )
    .expect("attempt");

    advance_attempt(&storage, "att_cas", "CREATED", "STARTED", None).expect("created->started");
    assert!(
        advance_attempt(&storage, "att_cas", "CREATED", "RUNNING", None).is_err(),
        "a transition from a state the attempt has already left must be refused"
    );
    advance_attempt(&storage, "att_cas", "STARTED", "RUNNING", None).expect("started->running");
    advance_attempt(&storage, "att_cas", "RUNNING", "COMPLETED", Some("9"))
        .expect("running->completed");
}

#[test]
fn child_slots_are_bounded_by_the_durable_task_scope() {
    let mut storage = project_storage();
    add_ready(&storage, "task_children", 1, "1");
    make_leaseable(&storage, "task_children");
    let lease = lease_task(&mut storage, &lease_request("task_children")).expect("lease");

    // `make_leaseable` declares max_parallel_children = 2.
    reserve_child_slot(
        &mut storage,
        "res_1",
        "task_children",
        &lease.lease_id,
        lease.lease_version,
        "1",
        "9999",
    )
    .expect("first child slot");
    reserve_child_slot(
        &mut storage,
        "res_2",
        "task_children",
        &lease.lease_id,
        lease.lease_version,
        "1",
        "9999",
    )
    .expect("second child slot");
    assert!(
        reserve_child_slot(
            &mut storage,
            "res_3",
            "task_children",
            &lease.lease_id,
            lease.lease_version,
            "1",
            "9999",
        )
        .is_err(),
        "a third slot exceeds the declared max_parallel_children of 2"
    );
}

// ---------------------------------------------------------------------------------------------------
// Effective priority: a task inherits the urgency of what it unblocks
// ---------------------------------------------------------------------------------------------------

/// The selected tasks as `(task_id, own priority, effective priority)`, so a test can state both the order and
/// the derivation behind it.
fn effective(storage: &Storage, limit: usize) -> Vec<(String, i64, i64)> {
    select_schedulable_tasks(storage, PROJECT, limit)
        .expect("select")
        .into_iter()
        .map(|task| (task.task_id, task.priority, task.effective_priority))
        .collect()
}

#[test]
fn a_prerequisite_inherits_the_priority_of_the_task_waiting_on_it() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_ready(&storage, "task_urgent", 100, "1");
    add_ready(&storage, "task_middling", 50, "1");
    depend(&storage, "task_urgent", "task_prereq");

    assert_eq!(
        effective(&storage, 10),
        vec![
            ("task_prereq".to_string(), 1, 100),
            ("task_middling".to_string(), 50, 50),
        ],
        "the prerequisite keeps its own priority of 1 and is ordered by an effective 100"
    );
    assert_eq!(
        selected(&storage, 10),
        vec!["task_prereq", "task_middling"],
        "the low-priority prerequisite now outranks unrelated middling work, which it did not before"
    );
}

#[test]
fn inheritance_is_transitive_along_a_chain() {
    let storage = project_storage();
    add_ready(&storage, "task_chain_head", 100, "1");
    add_ready(&storage, "task_chain_mid", 1, "1");
    add_ready(&storage, "task_chain_tail", 1, "1");
    add_ready(&storage, "task_unrelated", 60, "1");
    // head waits on mid, mid waits on tail, so tail is what everything is ultimately blocked behind.
    depend(&storage, "task_chain_mid", "task_chain_tail");
    depend(&storage, "task_chain_head", "task_chain_mid");

    assert_eq!(
        effective(&storage, 10),
        vec![
            ("task_chain_tail".to_string(), 1, 100),
            ("task_unrelated".to_string(), 60, 60),
        ],
        "the head's priority reaches the tail through the middle link"
    );
}

#[test]
fn a_long_chain_propagates_the_head_priority_to_the_far_end() {
    let storage = project_storage();
    add_ready(&storage, "task_top", 500, "1");
    for index in 0..30 {
        add_ready(&storage, &format!("task_link_{index:02}"), 1, "1");
    }
    add_ready(&storage, "task_head", 1, "1");

    depend(&storage, "task_top", "task_link_00");
    for index in 0..29 {
        depend(
            &storage,
            &format!("task_link_{index:02}"),
            &format!("task_link_{:02}", index + 1),
        );
    }
    depend(&storage, "task_link_29", "task_head");

    assert_eq!(
        effective(&storage, 10),
        vec![("task_head".to_string(), 1, 500)],
        "a priority 31 edges away still reaches the only selectable task in the chain"
    );
}

#[test]
fn a_task_with_nothing_waiting_on_it_is_unaffected() {
    let storage = project_storage();
    add_ready(&storage, "task_a", 5, "1");
    add_ready(&storage, "task_b", 3, "1");
    add_ready(&storage, "task_c", 1, "1");

    assert_eq!(
        effective(&storage, 10),
        vec![
            ("task_a".to_string(), 5, 5),
            ("task_b".to_string(), 3, 3),
            ("task_c".to_string(), 1, 1),
        ],
        "with no dependency edges at all, effective priority is exactly own priority"
    );
}

#[test]
fn a_completed_dependent_does_not_raise_its_prerequisite() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    // Synthetic but deliberate: a task cannot normally complete while its prerequisite is still READY. It
    // isolates the rule being tested - a COMPLETED task is no longer waiting on anything - from every other
    // path through the selector.
    add_task(
        &storage,
        "task_finished",
        100,
        "COMPLETED",
        0,
        Some(WORKSPACE),
        "1",
        "1",
    );
    add_ready(&storage, "task_other", 50, "1");
    depend(&storage, "task_finished", "task_prereq");

    assert_eq!(
        effective(&storage, 10),
        vec![
            ("task_other".to_string(), 50, 50),
            ("task_prereq".to_string(), 1, 1),
        ],
        "a completed task no longer needs its prerequisite, so it must not raise it"
    );
}

#[test]
fn an_invalidated_dependent_does_not_raise_its_prerequisite() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_task(
        &storage,
        "task_abandoned",
        100,
        "INVALIDATED",
        0,
        Some(WORKSPACE),
        "1",
        "1",
    );
    add_ready(&storage, "task_other", 50, "1");
    depend(&storage, "task_abandoned", "task_prereq");

    assert_eq!(
        effective(&storage, 10),
        vec![
            ("task_other".to_string(), 50, 50),
            ("task_prereq".to_string(), 1, 1),
        ],
        "an invalidated task will never run, so it is not waiting and promotes nothing"
    );
}

#[test]
fn a_blocked_dependent_still_raises_its_prerequisite() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_task(
        &storage,
        "task_blocked",
        100,
        "BLOCKED",
        0,
        Some(WORKSPACE),
        "1",
        "1",
    );
    add_ready(&storage, "task_other", 50, "1");
    depend(&storage, "task_blocked", "task_prereq");

    assert_eq!(
        selected(&storage, 10),
        vec!["task_prereq", "task_other"],
        "a blocked task is still going to run, so the work it waits on keeps its urgency"
    );
}

#[test]
fn inheritance_does_not_disturb_the_tie_break_chain() {
    let storage = project_storage();
    // Both prerequisites inherit 10, so they tie on effective priority and must fall back to the existing
    // updated_at / created_at / task_id ordering rather than to the order they were discovered in.
    add_task(
        &storage,
        "task_prereq_late",
        1,
        "READY",
        0,
        Some(WORKSPACE),
        "1",
        "9",
    );
    add_task(
        &storage,
        "task_prereq_early",
        1,
        "READY",
        0,
        Some(WORKSPACE),
        "1",
        "2",
    );
    add_ready(&storage, "task_waiting_a", 10, "1");
    add_ready(&storage, "task_waiting_b", 10, "1");
    depend(&storage, "task_waiting_a", "task_prereq_late");
    depend(&storage, "task_waiting_b", "task_prereq_early");

    assert_eq!(
        selected(&storage, 10),
        vec!["task_prereq_early", "task_prereq_late"],
        "equal effective priority still resolves by the original tie-break order"
    );
}

#[test]
fn inheritance_is_deterministic_across_databases_and_repeated_calls() {
    let build = || {
        let storage = project_storage();
        add_ready(&storage, "task_prereq", 1, "1");
        add_ready(&storage, "task_urgent", 100, "1");
        add_ready(&storage, "task_middling", 50, "1");
        add_ready(&storage, "task_low", 2, "1");
        depend(&storage, "task_urgent", "task_prereq");
        depend(&storage, "task_middling", "task_low");
        storage
    };
    let first = build();
    let second = build();

    assert_eq!(effective(&first, 10), effective(&second, 10));
    assert_eq!(effective(&first, 10), effective(&first, 10));
}

#[test]
fn promotion_can_lift_a_task_past_the_requested_limit() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_ready(&storage, "task_urgent", 100, "1");
    add_ready(&storage, "task_middling_a", 50, "1");
    add_ready(&storage, "task_middling_b", 49, "1");
    depend(&storage, "task_urgent", "task_prereq");

    // A limit of one must return the promoted prerequisite, not the highest own-priority candidate. Applying
    // the limit before promotion would return task_middling_a.
    assert_eq!(selected(&storage, 1), vec!["task_prereq"]);
}

#[test]
fn a_dependency_cycle_terminates_and_still_propagates() {
    let storage = project_storage();
    // The DAG validator rejects cycles before they reach storage. This asserts that selection neither hangs nor
    // panics if one is present anyway, and that the fixpoint still produces an answer.
    add_ready(&storage, "task_cycle_a", 1, "1");
    add_ready(&storage, "task_cycle_b", 2, "1");
    add_ready(&storage, "task_outside", 1, "1");
    depend(&storage, "task_cycle_a", "task_cycle_b");
    depend(&storage, "task_cycle_b", "task_cycle_a");
    // `task_outside` is the only task with no incomplete dependency, so it is the only candidate, and it is a
    // prerequisite of the cycle. Observing it proves the fixpoint ran over the cycle and finished.
    depend(&storage, "task_cycle_b", "task_outside");

    assert_eq!(
        effective(&storage, 10),
        vec![("task_outside".to_string(), 1, 2)],
        "the higher priority inside the cycle reaches the task outside it"
    );
    assert_eq!(effective(&storage, 10), effective(&storage, 10));
}

#[test]
fn a_self_dependency_terminates() {
    let storage = project_storage();
    add_ready(&storage, "task_self", 7, "1");
    depend(&storage, "task_self", "task_self");

    // A task that depends on itself can never satisfy its own dependency, so it is never selectable, and the
    // fixpoint over its edge must still terminate.
    assert!(selected(&storage, 10).is_empty());
}

#[test]
fn inheritance_does_not_leak_between_projects() {
    let storage = project_storage();
    add_ready(&storage, "task_prereq", 1, "1");
    add_ready(&storage, "task_urgent", 100, "1");
    depend(&storage, "task_urgent", "task_prereq");

    // A second project's task with the same shape must not be consulted. `prj_other` has no rows at all, so
    // this asserts the read is project-scoped rather than that it returns a particular value.
    let other = select_schedulable_tasks(&storage, "prj_other", 10).expect("select other");
    assert!(other.is_empty());
    assert_eq!(
        effective(&storage, 10),
        vec![("task_prereq".to_string(), 1, 100)]
    );
}

#[test]
fn selection_is_identical_with_and_without_agent_attempt_data_present() {
    // DEC-112: agent performance telemetry is informational only and must not influence scheduling. The strongest
    // available evidence for that is that the same inputs produce the same selection whether or not attempt data
    // exists, which cannot be true of a selector that reads it.
    let mut storage = project_storage();
    add_ready(&storage, "task_a", 5, "1");
    add_ready(&storage, "task_b", 1, "2");
    add_ready(&storage, "task_c", 1, "3");
    depend(&storage, "task_c", "task_b");
    make_leaseable(&storage, "task_b");
    let lease = lease_task(&mut storage, &lease_request("task_b")).expect("lease");

    // The quiet snapshot is taken after the lease and before any attempt. Leasing is a scheduling fact and is
    // held constant across both snapshots, so the only thing that changes between them is attempt history.
    let quiet_selection = selected(&storage, 10);
    let quiet_effective = effective(&storage, 10);
    assert_eq!(
        quiet_selection,
        vec!["task_a".to_string()],
        "the fixture must select something, or the comparison proves nothing"
    );

    // A full retry budget, including a failure and a timeout, which is the attempt history most likely to tempt a
    // selector into demoting the agent that produced it.
    for (attempt_id, attempt_no, state) in [
        ("att_q1", 1, "COMPLETED"),
        ("att_q2", 2, "FAILED"),
        ("att_q3", 3, "TIMED_OUT"),
    ] {
        start_attempt(
            &storage,
            &NewTaskAttempt {
                attempt_id: attempt_id.into(),
                task_id: "task_b".into(),
                project_id: PROJECT.into(),
                attempt_no,
                lease_id: lease.lease_id.clone(),
                agent_id: AGENT.into(),
                session_id: SESSION.into(),
                workspace_id: WORKSPACE.into(),
                fence_token: lease.lease_version,
                project_epoch: 0,
                context_snapshot_id: CONTEXT.into(),
                state: state.into(),
                checkpoint_id: None,
                failure_id: None,
                started_at: Some("1".into()),
                heartbeat_at: Some("1".into()),
                ended_at: Some("2".into()),
                created_at: "1".into(),
            },
        )
        .expect("attempt");
    }

    // The telemetry the selector would have to consult is present and non-empty, so its absence from the
    // selection below is a property of the selector rather than of an empty table.
    let counts = storage.agent_attempt_counts(PROJECT).expect("counts");
    assert_eq!(counts.len(), 1, "one agent has attempt history");
    assert_eq!(counts[0].agent_id, AGENT);
    assert_eq!(counts[0].sample_size, 3);
    assert_eq!(counts[0].by_state.len(), 3);

    assert_eq!(selected(&storage, 10), quiet_selection);
    assert_eq!(effective(&storage, 10), quiet_effective);
}
