//! MCF-v2 envelope validation.
//!
//! This is the first runtime slice of the protocol crate: one function that decides whether a JSON value is a
//! legal MCF-v2 envelope. It is deliberately tiny and deliberately adversarial.
//!
//! Two design constraints shape it. First, the allowed vocabulary is not written here - it is generated into
//! `generated::envelope` from `envelope.schema.json`, so the validator and the contract cannot disagree about
//! what is legal. Second, this is a hand-written subset rather than a JSON Schema evaluator, because the
//! repository has zero runtime dependencies and adding one to validate a single object would be the wrong
//! trade.
//!
//! What it does not do: it does not check that a payload matches the schema named for its message type, and it
//! does not make any authorization decision. It decides shape and vocabulary only. A valid envelope is
//! well-formed, not necessarily permitted - PolicyService owns that, and the bus must not collapse into it.

use serde_json::Value;

use crate::generated::envelope as vocab;

/// Why an envelope was refused.
///
/// Variants are per-rule so a caller can react programmatically, and each carries the offending value so a
/// failure is diagnosable without re-running with a debugger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeRejection {
    /// The value is not a JSON object.
    NotAnObject,
    /// A required field is absent.
    MissingField(&'static str),
    /// The envelope carries a field the contract does not define.
    UnknownField(String),
    /// A field holds a value outside the contract's vocabulary.
    InvalidValue { field: &'static str, detail: String },
    /// `protocol_version` names a version this build does not speak.
    UnsupportedProtocolVersion(String),
    /// A material-action message is missing the authorization context that authorises it.
    MissingAuthorizationContext(String),
    /// A field has the wrong JSON type.
    WrongType { field: &'static str, expected: &'static str },
}

impl std::fmt::Display for EnvelopeRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvelopeRejection::NotAnObject => write!(f, "envelope must be a JSON object"),
            EnvelopeRejection::MissingField(name) => write!(f, "envelope is missing required field `{name}`"),
            EnvelopeRejection::UnknownField(name) => write!(f, "envelope carries undefined field `{name}`"),
            EnvelopeRejection::InvalidValue { field, detail } => write!(f, "envelope field `{field}` is invalid: {detail}"),
            EnvelopeRejection::UnsupportedProtocolVersion(v) => {
                write!(f, "unsupported protocol version `{v}`; this build speaks {}", vocab::PROTOCOL_VERSION)
            }
            EnvelopeRejection::MissingAuthorizationContext(name) => write!(
                f,
                "`{name}` authorizes a material action and requires `{}`",
                vocab::MATERIAL_REQUIRED_FIELDS.join("`, `")
            ),
            EnvelopeRejection::WrongType { field, expected } => {
                write!(f, "envelope field `{field}` must be {expected}")
            }
        }
    }
}

impl std::error::Error for EnvelopeRejection {}

/// A validated envelope.
///
/// The raw value is retained rather than a typed struct with twenty-two fields, because this slice decides
/// legality; it does not claim to model the domain. Typing every field is the next slice's work, and doing it
/// now would mean writing two definitions of the same contract.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub value: Value,
}

impl Envelope {
    pub fn message_type(&self) -> &str {
        self.value.get("message_type").and_then(Value::as_str).unwrap_or_default()
    }

    pub fn project_id(&self) -> &str {
        self.value.get("project_id").and_then(Value::as_str).unwrap_or_default()
    }
}

/// Validate a JSON value as an MCF-v2 envelope.
///
/// Rejects, in order: a non-object; a wrong protocol version; a missing required field; an undefined field;
/// a field outside its vocabulary; a malformed identity, digest or recipient list; and a material-action
/// message that omits its authorization context. Every rejection is specific - a caller is never told merely
/// "invalid".
pub fn validate_envelope(value: &Value) -> Result<Envelope, EnvelopeRejection> {
    let obj = value.as_object().ok_or(EnvelopeRejection::NotAnObject)?;

    // Protocol version first: an envelope from a different protocol generation is not interpretable, so no
    // later rule would mean what it appears to mean.
    match obj.get("protocol_version").and_then(Value::as_str) {
        Some(v) if v == vocab::PROTOCOL_VERSION => {}
        Some(v) => return Err(EnvelopeRejection::UnsupportedProtocolVersion(v.to_string())),
        None => return Err(EnvelopeRejection::MissingField("protocol_version")),
    }

    for field in vocab::REQUIRED_FIELDS {
        if !obj.contains_key(*field) {
            return Err(EnvelopeRejection::MissingField(field));
        }
    }

    // additionalProperties:false in the contract. An unrecognised field is refused rather than ignored,
    // because ignoring it would let a sender smuggle meaning the receiver never sees validated.
    for key in obj.keys() {
        if !vocab::REQUIRED_FIELDS.contains(&key.as_str()) && !vocab::OPTIONAL_FIELDS.contains(&key.as_str()) {
            return Err(EnvelopeRejection::UnknownField(key.clone()));
        }
    }

    check_enum(obj, "channel", vocab::CHANNELS)?;
    check_enum(obj, "phase", vocab::PHASES)?;
    check_enum(obj, "message_type", vocab::MESSAGE_TYPES)?;
    check_enum(obj, "priority", vocab::PRIORITIES)?;
    check_schema_version(obj)?;
    check_non_negative_integer(obj, "sequence")?;
    check_non_negative_integer(obj, "project_epoch")?;
    check_identity(obj, "sender")?;
    check_recipients(obj)?;
    check_state_digest(obj)?;
    check_security(obj)?;

    // The conditional rule: a material action must arrive with the authorization context that permits it.
    // This is the envelope-level half of that; PolicyService independently verifies the values.
    let message_type = obj.get("message_type").and_then(Value::as_str).unwrap_or_default();
    if vocab::MATERIAL_ACTION_MESSAGE_TYPES.contains(&message_type) {
        for field in vocab::MATERIAL_REQUIRED_FIELDS {
            let present = obj.get(*field).map(|v| !v.is_null()).unwrap_or(false);
            if !present {
                return Err(EnvelopeRejection::MissingAuthorizationContext(message_type.to_string()));
            }
        }
        check_authorization_context(obj)?;
    }

    Ok(Envelope { value: value.clone() })
}

fn check_enum(
    obj: &serde_json::Map<String, Value>,
    field: &'static str,
    allowed: &[&str],
) -> Result<(), EnvelopeRejection> {
    match obj.get(field) {
        Some(Value::String(s)) if allowed.contains(&s.as_str()) => Ok(()),
        Some(Value::String(s)) => Err(EnvelopeRejection::InvalidValue {
            field,
            detail: format!("`{s}` is not one of the {} legal values", allowed.len()),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType { field, expected: "a string" }),
        None => Err(EnvelopeRejection::MissingField(field)),
    }
}

fn check_schema_version(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    match obj.get("schema_version").and_then(Value::as_str) {
        // The contract pins MAJOR to 2; MINOR and PATCH may move without breaking this build.
        Some(v) if v.split('.').count() == 3 && v.split('.').next().map(|m| m == "2").unwrap_or(false) => Ok(()),
        Some(v) => Err(EnvelopeRejection::InvalidValue {
            field: "schema_version",
            detail: format!("`{v}` does not match the required 2.x.y form"),
        }),
        None => Err(EnvelopeRejection::MissingField("schema_version")),
    }
}

fn check_non_negative_integer(obj: &serde_json::Map<String, Value>, field: &'static str) -> Result<(), EnvelopeRejection> {
    match obj.get(field) {
        Some(Value::Number(n)) if n.as_i64().is_some_and(|v| v >= 0) => Ok(()),
        Some(Value::Number(_)) => Err(EnvelopeRejection::InvalidValue {
            field,
            detail: "must be zero or greater".to_string(),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType { field, expected: "an integer" }),
        None => Err(EnvelopeRejection::MissingField(field)),
    }
}

/// An actor identity: a known actor kind, a non-empty id, and - for an agent - the agent type from DEC-029.
fn check_identity(obj: &serde_json::Map<String, Value>, field: &'static str) -> Result<(), EnvelopeRejection> {
    let identity = obj.get(field).ok_or(EnvelopeRejection::MissingField(field))?;
    let map = identity
        .as_object()
        .ok_or(EnvelopeRejection::WrongType { field, expected: "an identity object" })?;

    check_enum(map, "actor_type", vocab::ACTOR_TYPES)?;

    let id_empty = map.get("actor_id").and_then(Value::as_str).map(str::is_empty).unwrap_or(true);
    if id_empty {
        return Err(EnvelopeRejection::InvalidValue { field: field, detail: "actor_id must not be empty".into() });
    }

    // agent_type is optional, but when present it must name one of the three supported adapters. A retired
    // adapter cannot be reintroduced by an envelope that merely spells it.
    if let Some(agent_type) = map.get("agent_type") {
        match agent_type {
            Value::Null => {}
            Value::String(s) if vocab::AGENT_TYPES.contains(&s.as_str()) => {}
            Value::String(s) => {
                return Err(EnvelopeRejection::InvalidValue {
                    field: "agent_type",
                    detail: format!("`{s}` is not a supported agent type (DEC-029)"),
                })
            }
            _ => return Err(EnvelopeRejection::WrongType { field: "agent_type", expected: "a string or null" }),
        }
    }
    Ok(())
}

fn check_recipients(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    let recipients = obj.get("recipients").ok_or(EnvelopeRejection::MissingField("recipients"))?;
    let list = recipients
        .as_array()
        .ok_or(EnvelopeRejection::WrongType { field: "recipients", expected: "an array" })?;

    // minItems:1 in the contract. An envelope addressed to nobody is a routing defect, not a broadcast.
    if list.is_empty() {
        return Err(EnvelopeRejection::InvalidValue { field: "recipients", detail: "must name at least one recipient".into() });
    }
    for recipient in list {
        let map = recipient
            .as_object()
            .ok_or(EnvelopeRejection::WrongType { field: "recipients", expected: "identity objects" })?;
        check_enum(map, "actor_type", vocab::ACTOR_TYPES)?;
    }
    Ok(())
}

/// A state digest is 64 hex characters - a SHA-256 over JCS-canonical input (DEC-025). A malformed digest
/// would defeat staleness detection rather than merely look untidy.
fn check_state_digest(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    match obj.get("state_digest") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(s)) if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) => Ok(()),
        Some(Value::String(s)) => Err(EnvelopeRejection::InvalidValue {
            field: "state_digest",
            detail: format!("`{s}` is not 64 hex characters"),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType { field: "state_digest", expected: "a string or null" }),
    }
}

fn check_security(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    let security = obj.get("security").ok_or(EnvelopeRejection::MissingField("security"))?;
    let map = security
        .as_object()
        .ok_or(EnvelopeRejection::WrongType { field: "security", expected: "an object" })?;

    check_enum(map, "classification", vocab::CLASSIFICATIONS)?;

    // contains_secret_material carries `const: false` in the contract. An envelope asserting it carries secrets
    // is refused at the boundary rather than trusted to be handled safely further in.
    if let Some(flag) = map.get("contains_secret_material") {
        if flag != &Value::Bool(false) {
            return Err(EnvelopeRejection::InvalidValue {
                field: "contains_secret_material",
                detail: "must be false; secrets are referenced, never carried".into(),
            });
        }
    }
    Ok(())
}

fn check_authorization_context(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    let context = obj
        .get("authorization_context")
        .ok_or(EnvelopeRejection::MissingField("authorization_context"))?;
    let map = context
        .as_object()
        .ok_or(EnvelopeRejection::WrongType { field: "authorization_context", expected: "an object" })?;

    for key in ["lease_id", "lease_version", "workspace_id", "agent_id", "policy_scope", "capability_snapshot_id"] {
        let missing = match map.get(key) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty(),
            _ => false,
        };
        if missing {
            return Err(EnvelopeRejection::MissingAuthorizationContext("authorization_context".to_string()));
        }
    }

    // lease_version is 1-based in the contract; a zero or negative lease version is not a lease.
    match map.get("lease_version") {
        Some(Value::Number(n)) if n.as_i64().is_some_and(|v| v >= 1) => Ok(()),
        _ => Err(EnvelopeRejection::InvalidValue {
            field: "lease_version",
            detail: "must be an integer of 1 or more".into(),
        }),
    }
}