//! Envelope validation tests.
//!
//! Two halves. The accept side proves a legal envelope is not refused, because a validator that rejects
//! everything is trivially safe and useless. The reject side proves each rule actually bites, because a
//! validator that accepts everything is worse than none.
//!
//! Fixtures are built from the generated vocabulary rather than hard-coded strings, so a contract change that
//! adds a channel or a message type does not make these tests fail for the wrong reason.

use mayasaba_protocol::envelope::{validate_envelope, EnvelopeRejection};
use mayasaba_protocol::generated::envelope as vocab;
use serde_json::{json, Value};

/// A complete, legal envelope for a non-material message type.
fn valid() -> Value {
    json!({
        "protocol_version": "MCF-2",
        "schema_version": "2.0.0",
        "message_id": "11111111-1111-4111-8111-111111111111",
        "event_id": "22222222-2222-4222-8222-222222222222",
        "project_id": "33333333-3333-4333-8333-333333333333",
        "session_id": "44444444-4444-4444-8444-444444444444",
        "sender": { "actor_type": "AGENT", "actor_id": "hermes-1", "agent_type": "HERMES_AGENT" },
        "recipients": [{ "actor_type": "MAYASABA", "actor_id": "controller" }],
        "channel": "project",
        "message_type": "HEARTBEAT",
        "phase": "UNSCOPED",
        "correlation_id": "55555555-5555-4555-8555-555555555555",
        "sequence": 1,
        "project_epoch": 0,
        "priority": "PROGRESS_HEARTBEAT",
        "created_at": "2026-10-04T00:00:00Z",
        "requires_ack": false,
        "requires_response": false,
        "blocking": false,
        "payload": {},
        "security": { "classification": "INTERNAL_PROJECT", "secret_refs": [] },
        "operation_id": null
    })
}

fn authorization_context() -> Value {
    json!({
        "lease_id": "66666666-6666-4666-8666-666666666666",
        "lease_version": 1,
        "workspace_id": "77777777-7777-4777-8777-777777777777",
        "agent_id": "88888888-8888-4888-8888-888888888888",
        "policy_scope": "task:42",
        "required_capabilities": [],
        "capability_snapshot_id": "99999999-9999-4999-8999-999999999999"
    })
}

/// A material-action envelope, which the contract requires to carry full authorization.
fn valid_material() -> Value {
    let mut v = valid();
    v["message_type"] = json!("EXECUTION_REQUEST");
    v["channel"] = json!("execution");
    v["task_id"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    v["context_snapshot_id"] = json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    v["state_digest"] = json!("c".repeat(64));
    v["idempotency_key"] = json!("op-1");
    v["authorization_context"] = authorization_context();
    v["operation_id"] = json!("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
    v
}

#[test]
fn a_legal_envelope_is_accepted() {
    let envelope = validate_envelope(&valid()).expect("a legal envelope must be accepted");
    assert_eq!(envelope.message_type(), "HEARTBEAT");
    assert_eq!(envelope.project_id(), "33333333-3333-4333-8333-333333333333");
}

#[test]
fn a_legal_material_action_envelope_is_accepted() {
    let envelope = validate_envelope(&valid_material()).expect("a legal material envelope must be accepted");
    assert_eq!(envelope.message_type(), "EXECUTION_REQUEST");
}

#[test]
fn every_optional_field_may_be_present() {
    let mut v = valid();
    v["causation_id"] = json!("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
    v["round_id"] = json!("ffffffff-ffff-4fff-8fff-ffffffffffff");
    v["expires_at"] = json!("2026-10-05T00:00:00Z");
    v["task_id"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    v["context_snapshot_id"] = json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    v["state_digest"] = json!("a".repeat(64));
    v["idempotency_key"] = json!("op-1");
    validate_envelope(&v).expect("every contract-defined optional field must be accepted");
}

// ---------------------------------------------------------------- reject side

#[test]
fn a_non_object_is_rejected() {
    assert_eq!(validate_envelope(&json!("nope")).err(), Some(EnvelopeRejection::NotAnObject));
    assert_eq!(validate_envelope(&json!([])).err(), Some(EnvelopeRejection::NotAnObject));
    assert_eq!(validate_envelope(&json!(null)).err(), Some(EnvelopeRejection::NotAnObject));
}

#[test]
fn a_foreign_protocol_version_is_rejected() {
    let mut v = valid();
    v["protocol_version"] = json!("MCF-1");
    assert!(matches!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::UnsupportedProtocolVersion(_))
    ));
}

#[test]
fn a_missing_required_field_is_rejected_by_name() {
    // Every required field is checked, not just the first few.
    for field in vocab::REQUIRED_FIELDS {
        let mut v = valid();
        v.as_object_mut().unwrap().remove(*field);
        match validate_envelope(&v) {
            Err(EnvelopeRejection::MissingField(name)) => assert_eq!(name, *field),
            other => panic!("removing required field `{field}` must be reported by name, got {other:?}"),
        }
    }
}

#[test]
fn an_undefined_field_is_rejected() {
    // additionalProperties:false. Ignoring an unknown field would let a sender smuggle meaning the receiver
    // never validated.
    let mut v = valid();
    v["not_a_contract_field"] = json!("surprise");
    assert!(matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::UnknownField(_))));
}

#[test]
fn an_unknown_message_type_is_rejected() {
    let mut v = valid();
    v["message_type"] = json!("DO_ANYTHING");
    assert!(matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::InvalidValue { field: "message_type", .. })));
}

#[test]
fn an_unknown_channel_or_phase_or_priority_is_rejected() {
    for (field, bad) in [("channel", "nowhere"), ("phase", "NOT_A_PHASE"), ("priority", "WHENEVER")] {
        let mut v = valid();
        v[field] = json!(bad);
        assert!(
            matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::InvalidValue { field: f, .. }) if f == field),
            "`{field}`: `{bad}` must be rejected"
        );
    }
}

#[test]
fn a_bad_schema_version_is_rejected() {
    for bad in ["1.0.0", "2.0", "two.0.zero", ""] {
        let mut v = valid();
        v["schema_version"] = json!(bad);
        assert!(
            validate_envelope(&v).is_err(),
            "schema_version `{bad}` must be rejected"
        );
    }
    // MAJOR 2 with any MINOR/PATCH is legal.
    let mut v = valid();
    v["schema_version"] = json!("2.17.3");
    validate_envelope(&v).expect("2.x.y must be accepted");
}

#[test]
fn a_negative_sequence_or_epoch_is_rejected() {
    for field in ["sequence", "project_epoch"] {
        let mut v = valid();
        v[field] = json!(-1);
        assert!(validate_envelope(&v).is_err(), "a negative `{field}` must be rejected");
    }
    // Zero is the minimum and is legal.
    let mut v = valid();
    v["sequence"] = json!(0);
    v["project_epoch"] = json!(0);
    validate_envelope(&v).expect("zero sequence and epoch are legal");
}

#[test]
fn an_envelope_with_no_recipients_is_rejected() {
    let mut v = valid();
    v["recipients"] = json!([]);
    assert!(matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::InvalidValue { field: "recipients", .. })));
}

#[test]
fn a_withdrawn_agent_type_is_rejected() {
    // Claude Code and Cline were withdrawn by DEC-029. An envelope naming one must not be accepted merely
    // because the string is well formed.
    for retired in ["CLAUDE_CODE", "CLINE"] {
        let mut v = valid();
        v["sender"] = json!({ "actor_type": "AGENT", "actor_id": "x", "agent_type": retired });
        assert!(
            matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::InvalidValue { field: "agent_type", .. })),
            "retired agent type `{retired}` must be rejected"
        );
    }
    // Every retained agent type is accepted.
    for supported in vocab::AGENT_TYPES {
        let mut v = valid();
        v["sender"] = json!({ "actor_type": "AGENT", "actor_id": "x", "agent_type": supported });
        validate_envelope(&v).expect("every supported agent type must be accepted");
    }
}

#[test]
fn an_unknown_actor_kind_is_rejected() {
    let mut v = valid();
    v["sender"] = json!({ "actor_type": "ROBOT", "actor_id": "x" });
    assert!(validate_envelope(&v).is_err(), "an unknown actor kind must be rejected");
}

#[test]
fn a_malformed_state_digest_is_rejected() {
    let bad_digests: Vec<String> = vec!["short".into(), "z".repeat(64), "a".repeat(63), "a".repeat(65)];
    for bad in &bad_digests {
        let mut v = valid();
        v["state_digest"] = json!(bad);
        assert!(validate_envelope(&v).is_err(), "malformed state_digest `{bad}` must be rejected");
    }
    // Uppercase hex is legal; the contract allows both cases.
    let mut v = valid();
    v["state_digest"] = json!("A".repeat(64));
    validate_envelope(&v).expect("uppercase hex digest is legal");
}

#[test]
fn an_envelope_asserting_it_carries_secrets_is_rejected() {
    // contains_secret_material carries `const: false` in the contract. Secrets are referenced, never carried.
    let mut v = valid();
    v["security"]["contains_secret_material"] = json!(true);
    assert!(validate_envelope(&v).is_err(), "an envelope claiming to carry secrets must be rejected");
}

#[test]
fn a_material_action_without_authorization_is_rejected() {
    // Each authorization field is required in turn. None of these six is in the envelope's unconditional
    // required set - `operation_id`, the one that used to be, is conditional in the contract - so every removal
    // is caught by the conditional's own rule rather than by the base shape check.
    for field in vocab::MATERIAL_REQUIRED_FIELDS {
        let mut v = valid_material();
        v.as_object_mut().unwrap().remove(*field);
        match validate_envelope(&v) {
            Err(EnvelopeRejection::MissingAuthorizationContext(name)) => assert_eq!(name, "EXECUTION_REQUEST"),
            Err(EnvelopeRejection::MissingField(name)) => assert_eq!(name, *field),
            other => panic!("removing `{field}` from a material envelope must be refused, got {other:?}"),
        }
    }
}

#[test]
fn a_material_action_with_a_null_authorization_is_rejected() {
    // Present-but-null is not present. A context explicitly set to null must not satisfy the requirement.
    let mut v = valid_material();
    v["authorization_context"] = Value::Null;
    assert!(validate_envelope(&v).is_err(), "a null authorization_context must be rejected");
}

#[test]
fn a_zero_lease_version_is_rejected() {
    // The contract makes lease_version 1-based; zero is not a lease.
    let mut v = valid_material();
    v["authorization_context"]["lease_version"] = json!(0);
    assert!(validate_envelope(&v).is_err(), "lease_version 0 must be rejected");
    v["authorization_context"]["lease_version"] = json!(1);
    validate_envelope(&v).expect("lease_version 1 must be accepted");
}

#[test]
fn a_non_material_message_may_omit_the_authorization_context() {
    // The conditional must not over-apply: an ordinary message carries no lease and is still legal.
    let v = valid();
    assert!(v.get("authorization_context").is_none());
    validate_envelope(&v).expect("a non-material message needs no authorization context");
}

#[test]
fn every_material_action_type_is_actually_gated() {
    // If a new material type were added to the contract but not gated here, this would catch it. The gate
    // derives its list from the contract, so this test is what proves the derivation is honoured.
    for message_type in vocab::MATERIAL_ACTION_MESSAGE_TYPES {
        let mut v = valid();
        v["message_type"] = json!(message_type);
        // Drop every authorization field: the envelope must be refused because it is a material action
        // carrying no lease, workspace, snapshot or digest to authorise against.
        for field in vocab::MATERIAL_REQUIRED_FIELDS {
            v.as_object_mut().unwrap().remove(*field);
        }
        assert!(
            matches!(
                validate_envelope(&v).err(),
                Some(EnvelopeRejection::MissingAuthorizationContext(_)) | Some(EnvelopeRejection::MissingField(_))
            ),
            "`{message_type}` is declared a material action and must require authorization"
        );
    }
}

#[test]
fn the_generated_vocabulary_matches_the_contract_size() {
    // A sanity floor. If the generator silently emitted an empty list, every rejection above would still pass
    // while the validator accepted nothing.
    assert!(vocab::MESSAGE_TYPES.len() >= 50, "message types must come from the contract");
    assert!(vocab::CHANNELS.len() >= 15);
    assert!(vocab::PHASES.len() >= 20);
    assert!(vocab::MATERIAL_ACTION_MESSAGE_TYPES.len() >= 20);
    assert_eq!(vocab::PROTOCOL_VERSION, "MCF-2");
}

// ------------------------------------------- nested objects and the type table
//
// Each test below reproduces a divergence that existed between this validator and envelope.schema.json: the
// contract required something the hand-written logic did not check, because the field list had been copied by
// hand instead of generated. They are the regression guard for that whole class, not for one field.

#[test]
fn a_security_block_without_secret_refs_is_rejected() {
    // security.required = ["classification","secret_refs"].
    let mut v = valid();
    v["security"] = json!({ "classification": "INTERNAL_PROJECT" });
    assert_eq!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::MissingNestedField { object: "security", field: "secret_refs" })
    );
}

#[test]
fn an_authorization_context_without_required_capabilities_is_rejected() {
    // authorization_context.required names seven fields. The validator previously checked six, so an envelope
    // could authorize a material action without ever stating the capabilities PolicyService consumes.
    let mut v = valid_material();
    v["authorization_context"]
        .as_object_mut()
        .unwrap()
        .remove("required_capabilities");
    assert_eq!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::MissingNestedField {
            object: "authorization_context",
            field: "required_capabilities",
        })
    );
}

#[test]
fn a_recipient_without_actor_id_is_rejected() {
    // identity.required = ["actor_type","actor_id"], and a recipient is a full identity. The validator
    // previously checked only actor_type on recipients while requiring actor_id on the sender.
    let mut v = valid();
    v["recipients"] = json!([{ "actor_type": "MAYASABA" }]);
    assert_eq!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::MissingNestedField { object: "recipients[]", field: "actor_id" })
    );
}

#[test]
fn an_undefined_field_inside_a_nested_object_is_rejected() {
    // additionalProperties:false is declared on identity, on authorization_context and on security, not only on
    // the envelope. Ignoring an unknown nested field would let a sender smuggle meaning past the receiver.
    let mut v = valid();
    v["sender"] = json!({ "actor_type": "AGENT", "actor_id": "x", "smuggled": 1 });
    assert_eq!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::UnknownNestedField { object: "sender", field: "smuggled".into() })
    );

    let mut v = valid_material();
    v["authorization_context"]["smuggled"] = json!(1);
    assert_eq!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::UnknownNestedField {
            object: "authorization_context",
            field: "smuggled".into(),
        })
    );

    let mut v = valid();
    v["security"] = json!({ "classification": "INTERNAL_PROJECT", "secret_refs": [], "smuggled": 1 });
    assert_eq!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::UnknownNestedField { object: "security", field: "smuggled".into() })
    );
}

#[test]
fn a_schema_version_with_non_numeric_parts_is_rejected() {
    // The contract's pattern is ^2\.\d+\.\d+$ - digits, not merely three dot-separated parts. `2.x.y` was
    // previously accepted because only the part count and the major were checked.
    for bad in ["2.x.y", "2.1.x", "2..0", "2.1.", "2.1.0.0", "2.-1.0"] {
        let mut v = valid();
        v["schema_version"] = json!(bad);
        assert!(
            matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::InvalidValue { field: "schema_version", .. })),
            "schema_version `{bad}` must be rejected"
        );
    }
    let mut v = valid();
    v["schema_version"] = json!("2.17.3");
    validate_envelope(&v).expect("numeric MINOR and PATCH under the pinned MAJOR must be accepted");
}

#[test]
fn a_material_action_may_not_null_its_authorization_fields() {
    // The contract's `then` narrows every conditional field to a non-nullable type, so present-but-null is not
    // satisfied. Only authorization_context was narrowed before; the other five were enforced here alone, which
    // is the split that let the schema and the validator describe different languages.
    for field in vocab::MATERIAL_REQUIRED_FIELDS {
        let mut v = valid_material();
        v[*field] = Value::Null;
        assert!(
            matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::MissingAuthorizationContext(_))),
            "a material action with `{field}` explicitly null must be refused"
        );
    }
}

#[test]
fn operation_id_is_required_of_a_material_action_and_optional_otherwise() {
    // DEC-027 keys material-action idempotency by project_id + operation_id, and MCF-V2-PROTOCOL.md classifies
    // operation_id as a conditional field. It was previously in the envelope's unconditional required set,
    // which made the validator reject every non-material envelope that omitted it.
    assert!(
        !vocab::REQUIRED_FIELDS.contains(&"operation_id"),
        "operation_id is conditional, not unconditional"
    );
    assert!(vocab::OPTIONAL_FIELDS.contains(&"operation_id"));

    let mut v = valid();
    v.as_object_mut().unwrap().remove("operation_id");
    validate_envelope(&v).expect("a non-material envelope need not carry operation_id");

    let mut v = valid_material();
    v.as_object_mut().unwrap().remove("operation_id");
    assert!(matches!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::MissingAuthorizationContext(_))
    ));
}

#[test]
fn a_field_of_the_wrong_json_type_is_rejected() {
    // The type table is generated from each field's `type`, so a wrong type is caught for every field the
    // contract types - not only for the handful that had a hand-written check.
    for (field, bad) in [
        ("blocking", json!("yes")),
        ("requires_ack", json!(1)),
        ("payload", json!("not an object")),
        ("sequence", json!("1")),
        ("recipients", json!("controller")),
        ("created_at", json!(1234)),
        ("task_id", json!(12)),
    ] {
        let mut v = valid();
        v[field] = bad.clone();
        assert!(
            matches!(validate_envelope(&v).err(), Some(EnvelopeRejection::WrongJsonType { field: f, .. }) if f == field),
            "`{field}` set to {bad} must be refused as the wrong JSON type"
        );
    }
}

#[test]
fn a_nullable_field_still_accepts_null() {
    // The other side of the type table: a field the contract declares nullable must not become required-non-null
    // merely because it is typed. Over-rejecting is the same defect as under-rejecting.
    let mut v = valid();
    for field in ["causation_id", "task_id", "round_id", "context_snapshot_id", "state_digest", "idempotency_key", "expires_at", "operation_id"] {
        v[field] = Value::Null;
    }
    v["authorization_context"] = Value::Null;
    validate_envelope(&v).expect("every nullable field must accept null");
}

#[test]
fn a_repeated_array_entry_is_rejected() {
    // uniqueItems:true on secret_refs and required_capabilities.
    let mut v = valid();
    v["security"]["secret_refs"] = json!(["vault://a", "vault://a"]);
    assert!(matches!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::InvalidValue { field: "secret_refs", .. })
    ));

    let mut v = valid_material();
    v["authorization_context"]["required_capabilities"] = json!(["fs.write", "fs.write"]);
    assert!(matches!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::InvalidValue { field: "required_capabilities", .. })
    ));
}

#[test]
fn an_empty_idempotency_key_is_rejected() {
    // minLength:1 in the contract; an empty key is not a key.
    let mut v = valid();
    v["idempotency_key"] = json!("");
    assert!(matches!(
        validate_envelope(&v).err(),
        Some(EnvelopeRejection::InvalidValue { field: "idempotency_key", .. })
    ));
}
