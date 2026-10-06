//! Agent performance reporting (DEC-112): derived per-agent counts, a rate the reporting policy permits, and an
//! honest `UNAVAILABLE` where the repository has no data to compute a figure from.

use mayasaba_agents::{minimum_sample_for_percentage, AgentService, AgentStateCount};
use mayasaba_storage::{NewProject, NewTaskAttempt, Storage};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn project_storage() -> Storage {
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&NewProject {
            project_id: "prj_telemetry".to_string(),
            local_path: "C:\\work\\telemetry".to_string(),
            brief_id: "brf_telemetry".to_string(),
            brief_body: "Telemetry".to_string(),
            brief_source: "TEST".to_string(),
            event_id: "evt_telemetry".to_string(),
            created_at: "1".to_string(),
        })
        .expect("project");
    storage
}

/// Two agents, each owning its own task.
///
/// A live lease owns a task exclusively and an attempt's agent must match its lease's agent, so per-agent counts
/// cannot be produced from one lease. The fixture therefore has to give each agent a task of its own, which is
/// also the shape the real system has.
fn seed_two_agents(storage: &mut Storage) {
    for (agent, session) in [("agent_a", "sess_a"), ("agent_b", "sess_b")] {
        storage.conn().execute(
            &format!("INSERT INTO agents (agent_id, agent_type, executable, created_at, updated_at) VALUES ('{agent}','HERMES_AGENT','hermes','1','1')"),
            [],
        ).expect("agent");
        storage.conn().execute(
            &format!("INSERT INTO agent_sessions (session_id, project_id, agent_id, state, health_state, workspace_id, current_epoch, started_at) VALUES ('{session}','prj_telemetry','{agent}','READY','HEALTHY','ws_t',0,'1')"),
            [],
        ).expect("session");
    }
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id, project_id, kind, root_path, status, created_at) VALUES ('ws_t','prj_telemetry','AGENT','C:\\work\\telemetry','ACTIVE','1')",
        [],
    ).expect("workspace");
    storage.conn().execute(
        &format!("INSERT INTO context_snapshots (context_snapshot_id, project_id, epoch, scope, state_digest, pack_json, created_at) VALUES ('ctx_t','prj_telemetry',0,'TASK','{DIGEST}','{{}}','1')"),
        [],
    ).expect("context");
    for (task, agent, session, lease) in [
        ("task_a", "agent_a", "sess_a", "lease_a"),
        ("task_b", "agent_b", "sess_b", "lease_b"),
    ] {
        storage.conn().execute(
            &format!("INSERT INTO tasks (task_id, project_id, objective, status, priority, risk, workspace_id, current_epoch, created_at, updated_at) VALUES ('{task}','prj_telemetry','test','READY',1,'LOW','ws_t',0,'1','1')"),
            [],
        ).expect("task");
        storage.conn().execute(
            &format!("INSERT INTO task_scopes (task_id,allowed_paths_json,required_capabilities_json,validation_requirements_json,policy_scope,max_attempts,max_parallel_children,created_at,updated_at) VALUES ('{task}','[]','[]','[]','PROJECT_WRITE',8,2,'1','1')"),
            [],
        ).expect("task scope");
        storage.conn().execute(
            &format!("INSERT INTO task_leases (lease_id, task_id, project_id, agent_id, session_id, workspace_id, lease_version, project_epoch, context_snapshot_id, state_digest, allowed_paths_json, required_capabilities_json, policy_scope, issued_at, heartbeat_at, expires_at, status) VALUES ('{lease}','{task}','prj_telemetry','{agent}','{session}','ws_t',7,0,'ctx_t','{DIGEST}','[]','[]','PROJECT_WRITE','1','1','9999','ACTIVE')"),
            [],
        ).expect("lease");
    }
    storage.conn().execute(
        "INSERT INTO failures (failure_id, project_id, task_id, fingerprint, category, packet_json, created_at) VALUES ('fail_1','prj_telemetry','task_a','fp_1','INTERNAL','{}','1')",
        [],
    ).expect("failure");
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    storage: &Storage,
    attempt_id: &str,
    task: &str,
    agent: &str,
    session: &str,
    lease: &str,
    no: i64,
    state: &str,
    failure_id: Option<&str>,
) {
    storage
        .insert_task_attempt(&NewTaskAttempt {
            attempt_id: attempt_id.to_string(),
            task_id: task.to_string(),
            project_id: "prj_telemetry".to_string(),
            attempt_no: no,
            lease_id: lease.to_string(),
            agent_id: agent.to_string(),
            session_id: session.to_string(),
            workspace_id: "ws_t".to_string(),
            fence_token: 7,
            project_epoch: 0,
            context_snapshot_id: "ctx_t".to_string(),
            state: state.to_string(),
            checkpoint_id: None,
            failure_id: failure_id.map(str::to_string),
            started_at: Some("1".to_string()),
            heartbeat_at: Some("1".to_string()),
            ended_at: Some("2".to_string()),
            created_at: "1".to_string(),
        })
        .expect("attempt");
}

#[test]
fn counts_are_derived_per_agent_and_a_rate_needs_the_policy_minimum() {
    let minimum = minimum_sample_for_percentage().expect("the reporting policy");
    // A precondition of this test rather than a restatement of the policy: it needs one agent exactly at the
    // threshold and one below it, and a minimum of one leaves no room below.
    assert!(
        minimum >= 2,
        "the reporting policy minimum is {minimum}; this test needs a sample size strictly below it"
    );

    let mut storage = project_storage();
    seed_two_agents(&mut storage);

    // Exactly at the threshold, one attempt of which recorded a failure. An off-by-one here would silently
    // withhold a rate the policy permits.
    for no in 1..=minimum {
        attempt(
            &storage,
            &format!("att_a{no}"),
            "task_a",
            "agent_a",
            "sess_a",
            "lease_a",
            no,
            "COMPLETED",
            if no == 1 { Some("fail_1") } else { None },
        );
    }
    // One short of the threshold, so its rate must be withheld while its counts are still reported.
    for no in 1..minimum {
        attempt(
            &storage,
            &format!("att_b{no}"),
            "task_b",
            "agent_b",
            "sess_b",
            "lease_b",
            no,
            "FAILED",
            Some("fail_1"),
        );
    }

    let service = AgentService::new(storage);
    let reports = service
        .agent_performance_report("prj_telemetry")
        .expect("report");
    assert_eq!(reports.len(), 2, "one report per agent that has attempts");

    let a = reports
        .iter()
        .find(|r| r.agent_id == "agent_a")
        .expect("agent_a");
    assert_eq!(
        a.sample_size, minimum,
        "the sample size is every attempt, not only the failures"
    );
    assert_eq!(a.attempts_with_recorded_failure, 1);
    assert_eq!(
        a.by_state,
        vec![AgentStateCount {
            state: "COMPLETED".to_string(),
            count: minimum,
        }]
    );
    assert_eq!(a.failure_rate, Some(1.0 / minimum as f64));
    assert!(
        a.failure_rate_suppressed_reason.is_none(),
        "a reportable rate must carry no suppression reason"
    );

    let b = reports
        .iter()
        .find(|r| r.agent_id == "agent_b")
        .expect("agent_b");
    assert_eq!(b.sample_size, minimum - 1);
    assert_eq!(
        b.failure_rate, None,
        "a rate below the policy minimum must be withheld rather than shown"
    );
    assert!(
        b.failure_rate_suppressed_reason
            .as_deref()
            .unwrap_or_default()
            .contains("below the reporting policy minimum"),
        "the absence must be explained, not left to be read as zero: {:?}",
        b.failure_rate_suppressed_reason
    );
    // The counts are still reported for the agent whose rate was withheld: suppressing a rate is not suppressing
    // the data it would have been computed from.
    assert_eq!(b.by_state.iter().map(|s| s.count).sum::<i64>(), minimum - 1);
    assert_eq!(b.attempts_with_recorded_failure, minimum - 1);
}

#[test]
fn validation_survival_is_reported_unavailable_rather_than_inferred() {
    let mut storage = project_storage();
    seed_two_agents(&mut storage);
    attempt(
        &storage,
        "att_only",
        "task_a",
        "agent_a",
        "sess_a",
        "lease_a",
        1,
        "COMPLETED",
        None,
    );

    let service = AgentService::new(storage);
    let reports = service
        .agent_performance_report("prj_telemetry")
        .expect("report");
    let report = reports.first().expect("agent_a");

    // `validation_runs` carries `task_id` and no `attempt_id`, so an attempt cannot be attributed a validation
    // result. The figure is reported as unavailable rather than derived from task-level validation, which would
    // attribute one task's result to every attempt that ran against it (DEC-083).
    assert_eq!(report.validation_survival, "UNAVAILABLE");
    assert!(
        report.validation_survival_reason.contains("no attempt_id"),
        "the unavailability must name its cause: {}",
        report.validation_survival_reason
    );
    assert!(
        report.informational_only,
        "every report states the guarantee that is the point of the report"
    );
}

#[test]
fn an_agent_with_no_attempts_has_no_report_rather_than_a_zeroed_one() {
    let mut storage = project_storage();
    seed_two_agents(&mut storage);
    // Only agent_a ever runs. A report row for agent_b would be a row of zeroes, which reads as a measured zero
    // rather than as the absence of a measurement.
    attempt(
        &storage,
        "att_only",
        "task_a",
        "agent_a",
        "sess_a",
        "lease_a",
        1,
        "COMPLETED",
        None,
    );

    let service = AgentService::new(storage);
    let reports = service
        .agent_performance_report("prj_telemetry")
        .expect("report");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].agent_id, "agent_a");
}

#[test]
fn reporting_reads_the_threshold_from_the_policy_file() {
    // The threshold has one owner. If this stops resolving, the reporting policy was moved or renamed and the
    // consumer has to be pointed at it rather than a constant being written into the code.
    let minimum = minimum_sample_for_percentage().expect("the reporting policy");
    assert!(
        minimum > 0,
        "a minimum sample of {minimum} would permit any rate"
    );
}
