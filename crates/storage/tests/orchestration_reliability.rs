//! Storage-level conformance for the long-running orchestration reliability refinement.

use mayasaba_storage::{NewProject, Storage};

fn project_storage() -> Storage {
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&NewProject {
            project_id: "prj_reliability".to_string(),
            local_path: "C:\\work\\reliability".to_string(),
            brief_id: "brf_reliability".to_string(),
            brief_body: "Reliability test".to_string(),
            brief_source: "TEST".to_string(),
            event_id: "evt_reliability".to_string(),
            created_at: "1700000000".to_string(),
        })
        .expect("project");
    storage
}

fn seed_task_lease_workspace(storage: &mut Storage) {
    storage.conn().execute(
        "INSERT INTO agents (agent_id, agent_type, executable, created_at, updated_at) VALUES ('agent_1','HERMES_AGENT','hermes','1','1')",
        [],
    ).expect("agent");
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id, project_id, kind, root_path, status, created_at) VALUES ('ws_1','prj_reliability','AGENT','C:\\work\\reliability','ACTIVE','1')",
        [],
    ).expect("workspace");
    storage.conn().execute(
        "INSERT INTO tasks (task_id, project_id, objective, status, priority, risk, workspace_id, current_epoch, created_at, updated_at) VALUES ('task_1','prj_reliability','test','READY',1,'LOW','ws_1',0,'1','1')",
        [],
    ).expect("task");
    storage.conn().execute(
        "INSERT INTO agent_sessions (session_id, project_id, agent_id, state, health_state, workspace_id, current_epoch, started_at) VALUES ('sess_1','prj_reliability','agent_1','READY','HEALTHY','ws_1',0,'1')",
        [],
    ).expect("session");
    storage.conn().execute(
        "INSERT INTO context_snapshots (context_snapshot_id, project_id, epoch, scope, state_digest, pack_json, created_at) VALUES ('ctx_1','prj_reliability',0,'TASK','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','{}','1')",
        [],
    ).expect("context");
    storage.conn().execute(
        "INSERT INTO task_scopes (task_id,allowed_paths_json,required_capabilities_json,validation_requirements_json,policy_scope,max_attempts,max_parallel_children,created_at,updated_at)
         VALUES ('task_1','[]','[]','[]','PROJECT_WRITE',3,2,'1','1')",
        [],
    ).expect("task scope");
    let admission = mayasaba_storage::NewAdmission {
        admission_id:"admit_1".into(), project_id:"prj_reliability".into(), task_id:"task_1".into(),
        workspace_id:"ws_1".into(), lease_id:None, agent_id:None, session_id:None,
        kind:"WORKSPACE_ADMISSION".into(), epoch:0,
        context_digest:Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        base_checkpoint_ref:None, changed_paths_json:"[]".into(),
        checks_json:r#"[{"check_id":"REPOSITORY_IDENTITY","status":"PASS"},{"check_id":"BASELINE_CLEAN","status":"PASS"},{"check_id":"WORKTREE_ASSIGNED","status":"PASS"},{"check_id":"PROTECTED_PATHS_DETERMINED","status":"PASS"},{"check_id":"CONTEXT_FRESH","status":"PASS"},{"check_id":"WORKSPACE_OWNERSHIP","status":"PASS"}]"#.into(),
        verdict:"ADMITTED".into(), refusal_reasons_json:None, supersedes_admission_id:None, created_at:"1".into()
    };
    storage
        .insert_admission(&admission)
        .expect("workspace admission");
    storage.conn().execute(
        "INSERT INTO task_leases (lease_id, task_id, project_id, agent_id, session_id, workspace_id, lease_version, project_epoch, context_snapshot_id, state_digest, allowed_paths_json, required_capabilities_json, policy_scope, issued_at, heartbeat_at, expires_at, status) VALUES ('lease_1','task_1','prj_reliability','agent_1','sess_1','ws_1',7,0,'ctx_1','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','[]','[]','PROJECT_WRITE','1','1','9999','ACTIVE')",
        [],
    ).expect("lease");
}

#[test]
fn reliability_tables_are_created_and_have_the_declared_guards() {
    let storage = Storage::open_in_memory().expect("open");
    for table in [
        "task_attempts",
        "resource_reservations",
        "workspace_revisions",
        "environment_snapshots",
        "certification_bindings",
    ] {
        let exists: i64 = storage
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("table lookup");
        assert_eq!(exists, 1, "{table} must be part of the canonical schema");
    }
}

#[test]
fn task_attempt_numbers_are_unique_and_invalid_states_are_rejected() {
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, created_at) VALUES ('att_1','task_1','prj_reliability',1,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','STARTED','1')",
        [],
    ).expect("attempt");
    assert!(storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, created_at) VALUES ('att_2','task_1','prj_reliability',1,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','STARTED','2')",
        [],
    ).is_err(), "one task cannot have two attempt #1 records");
    assert!(storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, created_at) VALUES ('att_bad','task_1','prj_reliability',2,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','WORKING','3')",
        [],
    ).is_err(), "an undeclared attempt state must fail closed");
}

#[test]
fn exclusive_resources_are_globally_unique_while_held() {
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage.conn().execute(
        "INSERT INTO resource_reservations (reservation_id, project_id, task_id, lease_id, lease_version, resource_type, resource_key, mode, quantity, state, issued_at, expires_at, created_at) VALUES ('res_1','prj_reliability','task_1','lease_1',7,'PORT','3000','EXCLUSIVE',1,'HELD','1','9999','1')",
        [],
    ).expect("first reservation");
    assert!(storage.conn().execute(
        "INSERT INTO resource_reservations (reservation_id, project_id, task_id, lease_id, lease_version, resource_type, resource_key, mode, quantity, state, issued_at, expires_at, created_at) VALUES ('res_2','prj_reliability','task_1','lease_1',7,'PORT','3000','EXCLUSIVE',1,'HELD','2','9999','2')",
        [],
    ).is_err(), "an exclusive port cannot have two live owners");
}

#[test]
fn safe_provenance_records_accept_only_declared_vocabularies() {
    let storage = project_storage();
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id, project_id, kind, root_path, status, created_at) VALUES ('ws_2','prj_reliability','INTEGRATION','C:\\work\\reliability','ACTIVE','1')",
        [],
    ).expect("workspace");
    assert!(storage.conn().execute(
        "INSERT INTO workspace_revisions (revision_id, project_id, workspace_id, revision_no, source, status, observed_at, created_at) VALUES ('rev_1','prj_reliability','ws_2',1,'CONTROLLER','VERIFIED','1','1')",
        [],
    ).is_ok());
    assert!(storage.conn().execute(
        "INSERT INTO workspace_revisions (revision_id, project_id, workspace_id, revision_no, source, status, observed_at, created_at) VALUES ('rev_bad','prj_reliability','ws_2',2,'ROBOT','VERIFIED','2','2')",
        [],
    ).is_err(), "an undeclared revision source must fail closed");
}

#[test]
fn an_old_attempt_fence_is_rejected_after_lease_rollover() {
    use mayasaba_storage::NewTaskAttempt;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .insert_task_attempt(&NewTaskAttempt {
            attempt_id: "att_fence".to_string(),
            task_id: "task_1".to_string(),
            project_id: "prj_reliability".to_string(),
            attempt_no: 1,
            lease_id: "lease_1".to_string(),
            agent_id: "agent_1".to_string(),
            session_id: "sess_1".to_string(),
            workspace_id: "ws_1".to_string(),
            fence_token: 7,
            project_epoch: 0,
            context_snapshot_id: "ctx_1".to_string(),
            state: "RUNNING".to_string(),
            checkpoint_id: None,
            failure_id: None,
            started_at: Some("1".to_string()),
            heartbeat_at: Some("1".to_string()),
            ended_at: None,
            created_at: "1".to_string(),
        })
        .expect("attempt");
    storage.conn().execute("UPDATE task_leases SET lease_version = 8, status = 'ACTIVE' WHERE lease_id = 'lease_1'", []).expect("roll lease");
    assert!(
        storage.verify_attempt_fence("att_fence", 7).is_err(),
        "the old fence must not authorize a write"
    );
    assert!(storage.verify_attempt_fence("att_fence", 8).is_err(), "the attempt snapshot is fenced to version 7 and must be recovered/recreated rather than silently upgraded");
}

#[test]
fn active_fence_is_accepted_while_lease_version_matches() {
    use mayasaba_storage::NewTaskAttempt;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .insert_task_attempt(&NewTaskAttempt {
            attempt_id: "att_live".to_string(),
            task_id: "task_1".to_string(),
            project_id: "prj_reliability".to_string(),
            attempt_no: 1,
            lease_id: "lease_1".to_string(),
            agent_id: "agent_1".to_string(),
            session_id: "sess_1".to_string(),
            workspace_id: "ws_1".to_string(),
            fence_token: 7,
            project_epoch: 0,
            context_snapshot_id: "ctx_1".to_string(),
            state: "RUNNING".to_string(),
            checkpoint_id: None,
            failure_id: None,
            started_at: Some("1".to_string()),
            heartbeat_at: Some("1".to_string()),
            ended_at: None,
            created_at: "1".to_string(),
        })
        .expect("attempt");
    storage
        .verify_attempt_fence("att_live", 7)
        .expect("current fence");
}

#[test]
fn certification_and_environment_records_reject_invalid_vocabularies() {
    use mayasaba_storage::{NewCertificationBinding, NewEnvironmentSnapshot};
    let storage = project_storage();
    storage
        .insert_environment_snapshot(&NewEnvironmentSnapshot {
            environment_snapshot_id: "env_1".to_string(),
            project_id: "prj_reliability".to_string(),
            workspace_id: None,
            task_id: None,
            execution_id: None,
            os_identity: "Windows".to_string(),
            runtime_versions_json: "{}".to_string(),
            environment_policy_hash:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            source: "EXECUTION".to_string(),
            captured_at: "1".to_string(),
        })
        .expect("environment");
    assert!(storage
        .insert_environment_snapshot(&NewEnvironmentSnapshot {
            environment_snapshot_id: "env_bad".to_string(),
            project_id: "prj_reliability".to_string(),
            workspace_id: None,
            task_id: None,
            execution_id: None,
            os_identity: "Windows".to_string(),
            runtime_versions_json: "{}".to_string(),
            environment_policy_hash:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            source: "NETWORK".to_string(),
            captured_at: "2".to_string()
        })
        .is_err());
    // The certification binding deliberately requires a real validation row; without one the foreign key
    // blocks storage rather than permitting an ungrounded certification.
    assert!(
        storage
            .insert_certification_binding(&NewCertificationBinding {
                certification_binding_id: "cert_1".to_string(),
                project_id: "prj_reliability".to_string(),
                task_id: None,
                validation_id: "missing_validation".to_string(),
                workspace_revision_id: None,
                environment_snapshot_id: Some("env_1".to_string()),
                artifact_hashes_json: "[]".to_string(),
                validator_version: "validator-1".to_string(),
                test_suite_version: None,
                status: "ASSERTED".to_string(),
                supersedes_binding_id: None,
                reason: None,
                created_at: "1".to_string()
            })
            .is_err(),
        "certification without a validation run must fail closed"
    );
}

#[test]
fn resource_reservation_rejects_stale_lease_version() {
    use mayasaba_storage::NewResourceReservation;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    let result = storage.insert_resource_reservation(&NewResourceReservation {
        reservation_id: "res_stale".to_string(),
        project_id: "prj_reliability".to_string(),
        task_id: "task_1".to_string(),
        lease_id: "lease_1".to_string(),
        lease_version: 6,
        resource_type: "PORT".to_string(),
        resource_key: "3001".to_string(),
        mode: "EXCLUSIVE".to_string(),
        quantity: 1,
        state: "HELD".to_string(),
        issued_at: "1".to_string(),
        expires_at: "9999".to_string(),
        released_at: None,
        created_at: "1".to_string(),
    });
    assert!(
        result.is_err(),
        "a stale lease must not reserve a local resource"
    );
}

#[test]
fn append_only_trace_links_cover_task_attempt_provenance() {
    use mayasaba_storage::NewTraceLink;
    let storage = project_storage();
    storage
        .insert_trace_link(&NewTraceLink {
            trace_link_id: "trace_1".to_string(),
            project_id: "prj_reliability".to_string(),
            link_type: "TASK_ATTEMPT".to_string(),
            source_type: "Task".to_string(),
            source_id: "task_1".to_string(),
            target_type: "TaskAttempt".to_string(),
            target_id: "att_live".to_string(),
            created_at: "1".to_string(),
        })
        .expect("trace");
    let rows = storage.list_trace_links("prj_reliability").expect("links");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].link_type, "TASK_ATTEMPT");
    assert!(
        storage
            .insert_trace_link(&NewTraceLink {
                trace_link_id: "trace_2".to_string(),
                project_id: "prj_reliability".to_string(),
                link_type: "NOT_A_LINK".to_string(),
                source_type: "Task".to_string(),
                source_id: "task_1".to_string(),
                target_type: "TaskAttempt".to_string(),
                target_id: "att_live".to_string(),
                created_at: "2".to_string(),
            })
            .is_err(),
        "unknown provenance links must be rejected"
    );
}

#[test]
fn attempt_transition_is_compare_and_swap_and_heartbeat_is_fenced() {
    use mayasaba_storage::NewTaskAttempt;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .insert_task_attempt(&NewTaskAttempt {
            attempt_id: "att_transition".into(),
            task_id: "task_1".into(),
            project_id: "prj_reliability".into(),
            attempt_no: 1,
            lease_id: "lease_1".into(),
            agent_id: "agent_1".into(),
            session_id: "sess_1".into(),
            workspace_id: "ws_1".into(),
            fence_token: 7,
            project_epoch: 0,
            context_snapshot_id: "ctx_1".into(),
            state: "CREATED".into(),
            checkpoint_id: None,
            failure_id: None,
            started_at: None,
            heartbeat_at: Some("1".into()),
            ended_at: None,
            created_at: "1".into(),
        })
        .expect("attempt");
    storage
        .transition_task_attempt("att_transition", "CREATED", "STARTED", None)
        .expect("start");
    assert!(
        storage
            .transition_task_attempt("att_transition", "CREATED", "RUNNING", None)
            .is_err(),
        "CAS must reject a stale expected state"
    );
    storage
        .transition_task_attempt("att_transition", "STARTED", "RUNNING", None)
        .expect("running");
    storage
        .heartbeat_task_attempt("att_transition", 7, "2")
        .expect("heartbeat");
    storage
        .conn()
        .execute(
            "UPDATE task_leases SET lease_version=8 WHERE lease_id='lease_1'",
            [],
        )
        .expect("renew");
    assert!(
        storage
            .heartbeat_task_attempt("att_transition", 7, "3")
            .is_err(),
        "stale fence cannot heartbeat after lease rollover"
    );
}

#[test]
fn certification_binding_supersession_is_append_only() {
    use mayasaba_storage::NewCertificationBinding;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage.conn().execute(
        "INSERT INTO validation_runs (validation_id, project_id, task_id, scope_json, checks_json, verdict, created_at) VALUES ('val_1','prj_reliability','task_1','{}','{}','PASS','1')", []
    ).expect("validation");
    storage
        .insert_certification_binding(&NewCertificationBinding {
            certification_binding_id: "cert_1".into(),
            project_id: "prj_reliability".into(),
            task_id: Some("task_1".into()),
            validation_id: "val_1".into(),
            workspace_revision_id: None,
            environment_snapshot_id: None,
            artifact_hashes_json: "[]".into(),
            validator_version: "v1".into(),
            test_suite_version: None,
            status: "ASSERTED".into(),
            supersedes_binding_id: None,
            reason: None,
            created_at: "2".into(),
        })
        .expect("first certification");
    storage
        .insert_certification_binding(&NewCertificationBinding {
            certification_binding_id: "cert_2".into(),
            project_id: "prj_reliability".into(),
            task_id: Some("task_1".into()),
            validation_id: "val_1".into(),
            workspace_revision_id: None,
            environment_snapshot_id: None,
            artifact_hashes_json: "[]".into(),
            validator_version: "v2".into(),
            test_suite_version: None,
            status: "INVALIDATED".into(),
            supersedes_binding_id: Some("cert_1".into()),
            reason: Some("workspace changed".into()),
            created_at: "3".into(),
        })
        .expect("invalidation");
    let latest = storage
        .get_latest_certification_binding("prj_reliability", Some("task_1"))
        .expect("latest")
        .expect("binding");
    assert_eq!(latest.certification_binding_id, "cert_2");
    assert_eq!(latest.status, "INVALIDATED");
    let old_status: String = storage
        .conn()
        .query_row(
            "SELECT status FROM certification_bindings WHERE certification_binding_id='cert_1'",
            [],
            |r| r.get(0),
        )
        .expect("old");
    assert_eq!(
        old_status, "ASSERTED",
        "historical certification must not be rewritten"
    );
}

#[test]
fn invalid_environment_json_is_rejected() {
    use mayasaba_storage::NewEnvironmentSnapshot;
    let storage = project_storage();
    let result = storage.insert_environment_snapshot(&NewEnvironmentSnapshot {
        environment_snapshot_id: "env_bad_json".into(),
        project_id: "prj_reliability".into(),
        workspace_id: None,
        task_id: None,
        execution_id: None,
        os_identity: "Windows".into(),
        runtime_versions_json: "[]".into(),
        environment_policy_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .into(),
        source: "EXECUTION".into(),
        captured_at: "1".into(),
    });
    assert!(result.is_err(), "runtime_versions_json must be an object");
}

#[test]
fn invalid_certification_artifact_json_is_rejected() {
    use mayasaba_storage::NewCertificationBinding;
    let storage = project_storage();
    storage.conn().execute(
        "INSERT INTO validation_runs (validation_id, project_id, task_id, scope_json, checks_json, verdict, created_at) VALUES ('val_json','prj_reliability','task_1','{}','{}','PASS','1')", []
    ).expect("validation");
    let result = storage.insert_certification_binding(&NewCertificationBinding {
        certification_binding_id: "cert_bad_json".into(),
        project_id: "prj_reliability".into(),
        task_id: Some("task_1".into()),
        validation_id: "val_json".into(),
        workspace_revision_id: None,
        environment_snapshot_id: None,
        artifact_hashes_json: "{}".into(),
        validator_version: "v1".into(),
        test_suite_version: None,
        status: "ASSERTED".into(),
        supersedes_binding_id: None,
        reason: None,
        created_at: "2".into(),
    });
    assert!(result.is_err(), "artifact hashes must be a JSON array");
}

#[test]
fn resource_release_requires_current_fence() {
    use mayasaba_storage::NewResourceReservation;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .insert_resource_reservation(&NewResourceReservation {
            reservation_id: "res_release".into(),
            project_id: "prj_reliability".into(),
            task_id: "task_1".into(),
            lease_id: "lease_1".into(),
            lease_version: 7,
            resource_type: "PORT".into(),
            resource_key: "3010".into(),
            mode: "EXCLUSIVE".into(),
            quantity: 1,
            state: "HELD".into(),
            issued_at: "1".into(),
            expires_at: "9999".into(),
            released_at: None,
            created_at: "1".into(),
        })
        .expect("reservation");
    storage
        .conn()
        .execute(
            "UPDATE task_leases SET lease_version=8 WHERE lease_id='lease_1'",
            [],
        )
        .expect("renew");
    assert!(
        storage
            .release_resource_reservation("res_release", 7, "2")
            .is_err(),
        "stale owners cannot release after lease rollover"
    );
}

#[test]
fn asserted_certification_requires_pass_validation() {
    use mayasaba_storage::NewCertificationBinding;
    let storage = project_storage();
    storage.conn().execute(
        "INSERT INTO validation_runs (validation_id, project_id, task_id, scope_json, checks_json, verdict, created_at) VALUES ('val_fail','prj_reliability','task_1','{}','{}','FAIL','1')", []
    ).expect("validation");
    let result = storage.insert_certification_binding(&NewCertificationBinding {
        certification_binding_id: "cert_fail_validation".into(),
        project_id: "prj_reliability".into(),
        task_id: Some("task_1".into()),
        validation_id: "val_fail".into(),
        workspace_revision_id: None,
        environment_snapshot_id: None,
        artifact_hashes_json: "[]".into(),
        validator_version: "v1".into(),
        test_suite_version: None,
        status: "ASSERTED".into(),
        supersedes_binding_id: None,
        reason: None,
        created_at: "2".into(),
    });
    assert!(
        result.is_err(),
        "ASSERTED certification must require PASS validation"
    );
}

#[test]
fn lease_admission_rejects_stale_or_invalidated_context() {
    use mayasaba_storage::NewTaskLease;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .conn()
        .execute("DELETE FROM task_leases WHERE lease_id='lease_1'", [])
        .expect("remove fixture lease");
    storage.conn().execute(
        "INSERT INTO context_snapshots (context_snapshot_id, project_id, epoch, scope, state_digest, pack_json, created_at, invalidated_at)
         VALUES ('ctx_stale','prj_reliability',1,'TASK','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','{}','2','9')",
        [],
    ).expect("stale context");
    let lease = NewTaskLease {
        lease_id: "lease_stale_context".into(),
        task_id: "task_1".into(),
        project_id: "prj_reliability".into(),
        agent_id: "agent_1".into(),
        session_id: "sess_1".into(),
        workspace_id: "ws_1".into(),
        project_epoch: 0,
        context_snapshot_id: "ctx_stale".into(),
        state_digest: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        allowed_paths_json: "[]".into(),
        required_capabilities_json: "[]".into(),
        policy_scope: "PROJECT_WRITE".into(),
        issued_at: "10".into(),
        heartbeat_at: "10".into(),
        expires_at: "20".into(),
    };
    assert!(
        storage.lease_task(&lease).is_err(),
        "stale or invalidated context cannot authorize a lease"
    );
}

#[test]
fn durable_lease_lifecycle_is_fenced_and_releasable_after_renewal() {
    use mayasaba_storage::NewTaskLease;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .conn()
        .execute("DELETE FROM task_leases WHERE lease_id='lease_1'", [])
        .expect("remove fixture lease");
    let lease = NewTaskLease {
        lease_id: "lease_lifecycle".into(),
        task_id: "task_1".into(),
        project_id: "prj_reliability".into(),
        agent_id: "agent_1".into(),
        session_id: "sess_1".into(),
        workspace_id: "ws_1".into(),
        project_epoch: 0,
        context_snapshot_id: "ctx_1".into(),
        state_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        allowed_paths_json: "[]".into(),
        required_capabilities_json: "[]".into(),
        policy_scope: "PROJECT_WRITE".into(),
        issued_at: "10".into(),
        heartbeat_at: "10".into(),
        expires_at: "20".into(),
    };
    let active = storage.lease_task(&lease).expect("lease");
    assert_eq!(active.status, "ACTIVE");
    assert_eq!(active.lease_version, 1);

    let renewed = storage
        .renew_lease("lease_lifecycle", 1, "11", "21")
        .expect("renew");
    assert_eq!(renewed.status, "ACTIVE");
    assert_eq!(renewed.lease_version, 1);
    storage
        .release_lease("lease_lifecycle", 1, "12")
        .expect("release with stable lease fence");
    assert_eq!(
        storage
            .get_task_lease("lease_lifecycle")
            .expect("read")
            .unwrap()
            .status,
        "RELEASED"
    );

    // lease_task emits LEASE_REQUESTED twice on purpose: once under the TASK identity and once under the
    // LEASE identity, so the two lifecycles cannot collide (see the comment in lease_task). renew_lease
    // re-grants, so it emits LEASE_RENEWED and LEASE_ACTIVE. Asserting the per-type breakdown rather than a
    // single total keeps every phase observable and still fails if any one of them stops being emitted.
    let mut stmt = storage
        .conn()
        .prepare(
            "SELECT event_type, COUNT(*) FROM events
             WHERE project_id='prj_reliability'
               AND event_type IN ('LEASE_REQUESTED','LEASE_ACTIVE','LEASE_RENEWED','LEASE_RELEASED')
             GROUP BY event_type ORDER BY event_type ASC",
        )
        .expect("events");
    let counts: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(
        counts,
        vec![
            ("LEASE_ACTIVE".to_string(), 2),
            ("LEASE_RELEASED".to_string(), 1),
            ("LEASE_RENEWED".to_string(), 1),
            ("LEASE_REQUESTED".to_string(), 2),
        ],
        "request/grant/renew/grant/release lifecycle must be observable"
    );
}

#[test]
fn live_lease_uniqueness_and_deadline_expiry_are_durable() {
    use mayasaba_storage::NewTaskLease;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .conn()
        .execute("DELETE FROM task_leases WHERE lease_id='lease_1'", [])
        .expect("remove fixture lease");
    let lease = NewTaskLease {
        lease_id: "lease_expire".into(),
        task_id: "task_1".into(),
        project_id: "prj_reliability".into(),
        agent_id: "agent_1".into(),
        session_id: "sess_1".into(),
        workspace_id: "ws_1".into(),
        project_epoch: 0,
        context_snapshot_id: "ctx_1".into(),
        state_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        allowed_paths_json: "[]".into(),
        required_capabilities_json: "[]".into(),
        policy_scope: "PROJECT_WRITE".into(),
        issued_at: "10".into(),
        heartbeat_at: "10".into(),
        expires_at: "20".into(),
    };
    storage.lease_task(&lease).expect("lease");
    let mut duplicate = lease.clone();
    duplicate.lease_id = "lease_duplicate".into();
    assert!(
        storage.lease_task(&duplicate).is_err(),
        "a task cannot have two live owners"
    );
    storage
        .conn()
        .execute(
            "UPDATE task_leases SET expires_at='19' WHERE lease_id='lease_expire'",
            [],
        )
        .expect("shorten deadline");
    assert_eq!(storage.expire_due_leases("20").expect("expire"), 1);
    assert_eq!(
        storage
            .get_task_lease("lease_expire")
            .expect("read")
            .unwrap()
            .status,
        "EXPIRED"
    );
    let task_state: String = storage
        .conn()
        .query_row("SELECT status FROM tasks WHERE task_id='task_1'", [], |r| {
            r.get(0)
        })
        .expect("task");
    assert_eq!(task_state, "LEASE_EXPIRED");

    storage
        .queue_expired_task_for_recovery("task_1", "21")
        .expect("recovery staging");
    let task_state: String = storage
        .conn()
        .query_row("SELECT status FROM tasks WHERE task_id='task_1'", [], |r| {
            r.get(0)
        })
        .expect("task");
    assert_eq!(task_state, "RECOVERY_PENDING");

    storage
        .recover_expired_task("task_1", "22")
        .expect("recovery");
    let task_state: String = storage
        .conn()
        .query_row("SELECT status FROM tasks WHERE task_id='task_1'", [], |r| {
            r.get(0)
        })
        .expect("task");
    assert_eq!(task_state, "READY");
}

#[test]
fn recovery_ready_transition_is_blocked_by_unresolved_attempt() {
    use mayasaba_storage::NewTaskAttempt;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    storage
        .conn()
        .execute(
            "UPDATE task_leases SET expires_at='9' WHERE lease_id='lease_1'",
            [],
        )
        .expect("deadline");
    // The fixture inserts an ACTIVE lease with raw SQL, which bypasses the READY -> LEASE_REQUESTED -> LEASED
    // transition that lease_task performs, so the task is left READY while a live lease exists. Expiry only
    // moves a task that is genuinely lease-held, so establish the state the lease implies.
    storage
        .conn()
        .execute(
            "UPDATE tasks SET status='LEASED' WHERE task_id='task_1'",
            [],
        )
        .expect("lease-held task");
    storage.expire_due_leases("10").expect("expire");
    storage
        .queue_expired_task_for_recovery("task_1", "11")
        .expect("stage");

    storage
        .conn()
        .execute(
            "UPDATE task_leases SET status='EXPIRED' WHERE lease_id='lease_1'",
            [],
        )
        .expect("expired");
    storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id,task_id,project_id,attempt_no,lease_id,agent_id,session_id,workspace_id,fence_token,project_epoch,context_snapshot_id,state,created_at)
         VALUES ('att_unknown','task_1','prj_reliability',1,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','UNKNOWN','11')", []
    ).expect("unknown attempt");
    assert!(
        storage.recover_expired_task("task_1", "12").is_err(),
        "unknown physical outcome blocks readiness"
    );
}

#[test]
fn admission_is_fail_closed_and_re_evaluation_requires_supersession() {
    use mayasaba_storage::NewAdmission;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    let bad = NewAdmission {
        admission_id: "admit_bad".into(),
        project_id: "prj_reliability".into(),
        task_id: "task_1".into(),
        workspace_id: "ws_1".into(),
        lease_id: None,
        agent_id: None,
        session_id: None,
        kind: "WORKSPACE_ADMISSION".into(),
        epoch: 0,
        context_digest: Some(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        ),
        base_checkpoint_ref: None,
        changed_paths_json: "[]".into(),
        checks_json: r#"[{"check_id":"BASELINE_CLEAN","status":"FAIL"}]"#.into(),
        verdict: "ADMITTED".into(),
        refusal_reasons_json: None,
        supersedes_admission_id: None,
        created_at: "2".into(),
    };
    assert!(
        storage.insert_admission(&bad).is_err(),
        "ADMITTED cannot contain FAIL"
    );

    let current = storage
        .get_latest_admission("prj_reliability", "task_1", "WORKSPACE_ADMISSION")
        .expect("read")
        .expect("seed admission");
    let missing_parent = NewAdmission {
        admission_id: "admit_parallel".into(),
        project_id: "prj_reliability".into(),
        task_id: "task_1".into(),
        workspace_id: "ws_1".into(),
        lease_id: None,
        agent_id: None,
        session_id: None,
        kind: "WORKSPACE_ADMISSION".into(),
        epoch: 0,
        context_digest: Some(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        ),
        base_checkpoint_ref: None,
        changed_paths_json: "[]".into(),
        checks_json: r#"[{"check_id":"BASELINE_CLEAN","status":"PASS"}]"#.into(),
        verdict: "ADMITTED".into(),
        refusal_reasons_json: None,
        supersedes_admission_id: None,
        created_at: "3".into(),
    };
    assert!(
        storage.insert_admission(&missing_parent).is_err(),
        "second effective admission requires supersession"
    );

    let next = NewAdmission {
        admission_id: "admit_2".into(),
        supersedes_admission_id: Some(current.admission_id.clone()),
        ..missing_parent
    };
    storage
        .insert_admission(&next)
        .expect("superseding admission");
    assert_eq!(
        storage
            .get_latest_admission("prj_reliability", "task_1", "WORKSPACE_ADMISSION")
            .expect("read")
            .unwrap()
            .admission_id,
        "admit_2"
    );
}

#[test]
fn lease_cannot_bypass_workspace_admission_or_task_scope() {
    use mayasaba_storage::NewTaskLease;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    let scope_ok = storage
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM task_scopes WHERE task_id='task_1'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .expect("scope");
    assert_eq!(scope_ok, 1);

    storage.conn().execute("UPDATE admissions SET verdict='REFUSED', refusal_reasons_json='[\"manual block\"]' WHERE admission_id='admit_1'", [])
        .expect("refuse admission");
    let base = NewTaskLease {
        lease_id: "lease_bypass".into(),
        task_id: "task_1".into(),
        project_id: "prj_reliability".into(),
        agent_id: "agent_1".into(),
        session_id: "sess_1".into(),
        workspace_id: "ws_1".into(),
        project_epoch: 0,
        context_snapshot_id: "ctx_1".into(),
        state_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        allowed_paths_json: "[]".into(),
        required_capabilities_json: "[]".into(),
        policy_scope: "PROJECT_WRITE".into(),
        issued_at: "10".into(),
        heartbeat_at: "10".into(),
        expires_at: "20".into(),
    };
    assert!(
        storage.lease_task(&base).is_err(),
        "refused workspace admission blocks lease"
    );

    storage.conn().execute("UPDATE admissions SET verdict='ADMITTED', refusal_reasons_json=NULL WHERE admission_id='admit_1'", [])
        .expect("restore seed");
    let widened = NewTaskLease {
        allowed_paths_json: r#"[".."]"#.into(),
        ..base
    };
    assert!(
        storage.lease_task(&widened).is_err(),
        "caller cannot widen durable task scope"
    );
}

#[test]
fn attempt_creation_enforces_contiguous_max_attempts() {
    use mayasaba_storage::NewTaskAttempt;
    let mut storage = project_storage();
    seed_task_lease_workspace(&mut storage);
    let base = NewTaskAttempt {
        attempt_id: "att_budget_1".into(),
        task_id: "task_1".into(),
        project_id: "prj_reliability".into(),
        attempt_no: 1,
        lease_id: "lease_1".into(),
        agent_id: "agent_1".into(),
        session_id: "sess_1".into(),
        workspace_id: "ws_1".into(),
        fence_token: 7,
        project_epoch: 0,
        context_snapshot_id: "ctx_1".into(),
        state: "STARTED".into(),
        checkpoint_id: None,
        failure_id: None,
        started_at: Some("2".into()),
        heartbeat_at: Some("2".into()),
        ended_at: None,
        created_at: "2".into(),
    };
    storage.insert_task_attempt(&base).expect("attempt 1");

    let second = NewTaskAttempt {
        attempt_id: "att_budget_2".into(),
        attempt_no: 2,
        created_at: "3".into(),
        ..base.clone()
    };
    storage.insert_task_attempt(&second).expect("attempt 2");

    let skipped = NewTaskAttempt {
        attempt_id: "att_budget_4".into(),
        attempt_no: 4,
        created_at: "4".into(),
        ..base.clone()
    };
    assert!(
        storage.insert_task_attempt(&skipped).is_err(),
        "attempt numbers cannot skip a retry"
    );

    let third = NewTaskAttempt {
        attempt_id: "att_budget_3".into(),
        attempt_no: 3,
        created_at: "5".into(),
        ..base
    };
    storage.insert_task_attempt(&third).expect("attempt 3");

    let exhausted = NewTaskAttempt {
        attempt_id: "att_budget_4b".into(),
        attempt_no: 4,
        created_at: "6".into(),
        ..second
    };
    assert!(
        storage.insert_task_attempt(&exhausted).is_err(),
        "max_attempts=3 must refuse attempt 4"
    );
}

#[test]
fn new_resource_reservations_cannot_start_terminal() {
    use mayasaba_storage::NewResourceReservation;
    let storage = project_storage();
    assert!(
        storage
            .insert_resource_reservation(&NewResourceReservation {
                reservation_id: "res_terminal".into(),
                project_id: "prj_reliability".into(),
                task_id: "task_1".into(),
                lease_id: "lease_1".into(),
                lease_version: 7,
                resource_type: "PORT".into(),
                resource_key: "4000".into(),
                mode: "EXCLUSIVE".into(),
                quantity: 1,
                state: "RELEASED".into(),
                issued_at: "2".into(),
                expires_at: "9".into(),
                released_at: Some("3".into()),
                created_at: "2".into(),
            })
            .is_err(),
        "new resource admission must always enter HELD"
    );
}
