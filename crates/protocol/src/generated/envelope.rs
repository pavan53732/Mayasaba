// GENERATED FILE - DO NOT EDIT.
// Source: schemas/mcf-v2/envelope.schema.json, identity.schema.json, enums.schema.json
// Regenerate: npm run codegen:protocol
//
// The envelope's vocabulary is generated from the contract so the Rust validator and the schema cannot
// disagree about what a legal envelope is. The validation logic itself is hand-written in envelope.rs,
// because a generic JSON Schema evaluator would be a dependency this repository deliberately does not have.

/// The only protocol version this build speaks.
pub const PROTOCOL_VERSION: &str = "MCF-2";

/// Every legal channel.
pub const CHANNELS: &[&str] = &["project", "agent", "council", "requirement", "decision", "architecture", "context", "task", "lease", "workspace", "execution", "build", "test", "validation", "repair", "evidence", "review", "communication", "diagnostics"];

/// Every legal lifecycle phase, including UNSCOPED for messages outside a project phase.
pub const PHASES: &[&str] = &["UNSCOPED", "PROJECT_CREATED", "DISCOVERY", "INDEPENDENT_ANALYSIS", "PROPOSALS", "CROSS_CRITIQUE", "REBUTTAL_AND_REVISION", "DISAGREEMENT_RESOLUTION", "USER_INTERVIEW", "PRODUCT_AND_UX_DESIGN", "TECH_STACK_DEBATE", "ARCHITECTURE_REVIEW", "ARCHITECTURE_LOCKED", "TASK_PLANNING", "IMPLEMENTATION", "INTEGRATION", "BUILD", "TEST", "E2E", "CROSS_AGENT_REVIEW", "FINAL_VALIDATION", "PACKAGE", "COMPLETE"];

/// Every legal message type.
pub const MESSAGE_TYPES: &[&str] = &["HANDSHAKE", "HANDSHAKE_ACK", "READY", "HEARTBEAT", "ACK", "NACK", "PAUSE", "RESUME", "CANCEL", "STOP", "ERROR", "RETRY", "DEAD_LETTER", "SYNC_REQUEST", "SYNC_RESPONSE", "STATE_DIGEST", "CONTEXT_UPDATE", "STALE_CONTEXT", "EPOCH_CHANGED", "IDEA", "PROPOSAL", "QUESTION", "CRITIQUE", "COUNTERARGUMENT", "REBUTTAL", "REVISION", "AGREE", "DISAGREE", "BLOCK", "ACCEPT", "REJECT", "ABSTAIN", "DECISION", "LOCK", "SYNTHESIS", "TASK", "TASK_ACCEPT", "TASK_REJECT", "TASK_LEASE", "TASK_LEASE_RENEW", "TASK_RELEASE", "TASK_PROGRESS", "HANDOFF_REQUEST", "HANDOFF_ACCEPT", "HANDOFF_REJECT", "IMPLEMENTATION_REPORT", "FAILURE", "DIAGNOSIS", "REPAIR_REQUEST", "REPAIR_RESULT", "REVIEW", "TEST_RESULT", "VALIDATION", "CERTIFICATION", "EXECUTION_REQUEST", "EXECUTION_STARTED", "EXECUTION_RESULT", "ARTIFACT_PUBLISHED", "EVIDENCE_PUBLISHED"];

/// Message types that authorize a material action and therefore require the full authorization context.
pub const MATERIAL_ACTION_MESSAGE_TYPES: &[&str] = &["TASK_ACCEPT", "TASK_REJECT", "TASK_LEASE", "TASK_LEASE_RENEW", "TASK_RELEASE", "TASK_PROGRESS", "HANDOFF_REQUEST", "HANDOFF_ACCEPT", "HANDOFF_REJECT", "IMPLEMENTATION_REPORT", "FAILURE", "DIAGNOSIS", "REPAIR_REQUEST", "REPAIR_RESULT", "REVIEW", "TEST_RESULT", "VALIDATION", "CERTIFICATION", "EXECUTION_REQUEST", "EXECUTION_STARTED", "EXECUTION_RESULT", "ARTIFACT_PUBLISHED", "EVIDENCE_PUBLISHED"];

/// Fields every envelope must carry.
pub const REQUIRED_FIELDS: &[&str] = &["protocol_version", "schema_version", "message_id", "event_id", "project_id", "session_id", "sender", "recipients", "channel", "message_type", "phase", "correlation_id", "sequence", "project_epoch", "priority", "created_at", "requires_ack", "requires_response", "blocking", "payload", "security"];

/// Fields an envelope may carry. The schema sets additionalProperties:false, so anything else is rejected.
pub const OPTIONAL_FIELDS: &[&str] = &["causation_id", "task_id", "round_id", "context_snapshot_id", "state_digest", "idempotency_key", "expires_at", "authorization_context", "operation_id"];

/// The JSON type(s) each typed envelope field may hold, from the contract's own `type` keyword.
///
/// A field absent from this table declares no single `type` in the contract - it is a `$ref`, a bare `enum`
/// or a bare `const` - so its vocabulary is enforced by the hand-written rule named for it instead. The names
/// are resolved by holds_type() in envelope.rs.
pub const FIELD_TYPES: &[(&str, &[&str])] = &[("schema_version", &["Str"]), ("message_id", &["Str"]), ("event_id", &["Str"]), ("project_id", &["Str"]), ("session_id", &["Str"]), ("recipients", &["Arr"]), ("correlation_id", &["Str"]), ("causation_id", &["Str", "Null"]), ("sequence", &["Int"]), ("project_epoch", &["Int"]), ("created_at", &["Str"]), ("requires_ack", &["Bool"]), ("requires_response", &["Bool"]), ("blocking", &["Bool"]), ("task_id", &["Str", "Null"]), ("round_id", &["Str", "Null"]), ("context_snapshot_id", &["Str", "Null"]), ("state_digest", &["Str", "Null"]), ("idempotency_key", &["Str", "Null"]), ("expires_at", &["Str", "Null"]), ("authorization_context", &["Obj", "Null"]), ("payload", &["Obj"]), ("security", &["Obj"]), ("operation_id", &["Str", "Null"])];

/// Extra fields a material-action envelope must carry beyond the base set. The contract's conditional narrows
/// each of these to a non-nullable type in `then.properties`, so presence alone is not enough: a null value
/// does not satisfy it.
pub const MATERIAL_REQUIRED_FIELDS: &[&str] = &["task_id", "context_snapshot_id", "state_digest", "idempotency_key", "authorization_context", "operation_id"];

/// Fields every authorization_context must carry, from the contract's own nested `required`.
pub const AUTHORIZATION_CONTEXT_REQUIRED_FIELDS: &[&str] = &["lease_id", "lease_version", "workspace_id", "agent_id", "policy_scope", "required_capabilities", "capability_snapshot_id"];

/// Fields an authorization_context may carry. The contract sets additionalProperties:false on it.
pub const AUTHORIZATION_CONTEXT_FIELDS: &[&str] = &["lease_id", "lease_version", "workspace_id", "agent_id", "policy_scope", "required_capabilities", "capability_snapshot_id"];

/// Fields every security block must carry.
pub const SECURITY_REQUIRED_FIELDS: &[&str] = &["classification", "secret_refs"];

/// Fields a security block may carry. The contract sets additionalProperties:false on it.
pub const SECURITY_FIELDS: &[&str] = &["classification", "secret_refs", "contains_secret_material"];

/// Fields every actor identity must carry, applied to the sender and to every recipient alike.
pub const IDENTITY_REQUIRED_FIELDS: &[&str] = &["actor_type", "actor_id"];

/// Fields an actor identity may carry. The contract sets additionalProperties:false on it.
pub const IDENTITY_FIELDS: &[&str] = &["actor_type", "actor_id", "agent_id", "agent_type", "session_id"];

/// The MAJOR that schema_version must carry, derived from the contract's pattern.
pub const SCHEMA_VERSION_MAJOR: &str = "2";

/// Every legal delivery priority.
pub const PRIORITIES: &[&str] = &["EMERGENCY_CONTROL", "SYNCHRONIZATION", "TASK_CONTROL", "FAILURE_RECOVERY", "COUNCIL", "PROGRESS_HEARTBEAT", "BULK"];

/// Every legal security classification.
pub const CLASSIFICATIONS: &[&str] = &["PUBLIC_PROJECT", "INTERNAL_PROJECT", "SENSITIVE", "SECRET_REFERENCE_ONLY"];

/// Every legal actor kind.
pub const ACTOR_TYPES: &[&str] = &["MAYASABA", "AGENT"];

/// Every legal agent type, from the three supported adapters (DEC-029).
pub const AGENT_TYPES: &[&str] = &["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"];
