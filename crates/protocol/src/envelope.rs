//! MCF-v2 envelope validation.
//!
//! This is the first runtime slice of the protocol crate: one function that decides whether a JSON value is a
//! legal MCF-v2 envelope. It is deliberately tiny and deliberately adversarial.
//!
//! Two design constraints shape it. First, the allowed vocabulary is not written here - it is generated into
//! `generated::envelope` from `envelope.schema.json`, `identity.schema.json` and `enums.schema.json`, so the
//! validator and the contract cannot disagree about what is legal. Field sets, nested field sets and per-field
//! JSON types all come from the contract; none of them is hand-copied here. Second, this is a hand-written
//! subset rather than a JSON Schema evaluator, because the repository has zero runtime dependencies and adding
//! one to validate a single object would be the wrong trade.
//!
//! What the subset enforces: the presence of every contract-required field at the envelope and inside each
//! nested object; the JSON type of every field whose contract declares one; `additionalProperties:false` at
//! every level; and `enum`, `const`, `minimum`, `minLength`, `uniqueItems` and `pattern` where the contract
//! declares them. It also enforces the material-action conditional, including the fact that the conditional
//! narrows each of its fields to a non-nullable type - so a present-but-null value does not satisfy it.
//!
//! What the subset does NOT enforce, stated so the guarantee is not overstated: `format` annotations (`uuid`,
//! `date-time`). JSON Schema defines `format` as annotation-only by default, so treating it as a validation
//! keyword would reject envelopes the contract admits. A field whose contract is a `$ref`, a bare `enum` or a
//! bare `const` carries no `type`, so it is deliberately absent from the generated type table and is checked by
//! the hand-written rule named for it instead.
//!
//! What it does not do at all: it does not check that a payload matches the schema named for its message type,
//! and it does not make any authorization decision. It decides shape and vocabulary only. A valid envelope is
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
    /// A required field is absent from the envelope itself.
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
    WrongType {
        field: &'static str,
        expected: &'static str,
    },
    /// A field's JSON type is not among the types the contract permits for it.
    WrongJsonType {
        field: &'static str,
        got: &'static str,
        allowed: &'static [&'static str],
    },
    /// A required field is absent from a nested object the contract defines.
    MissingNestedField {
        object: &'static str,
        field: &'static str,
    },
    /// A nested object carries a field the contract does not define.
    UnknownNestedField { object: &'static str, field: String },
    /// The text is not JSON at all, so there is no envelope to validate.
    Malformed(String),
}

impl std::fmt::Display for EnvelopeRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvelopeRejection::NotAnObject => write!(f, "envelope must be a JSON object"),
            EnvelopeRejection::MissingField(name) => {
                write!(f, "envelope is missing required field `{name}`")
            }
            EnvelopeRejection::UnknownField(name) => {
                write!(f, "envelope carries undefined field `{name}`")
            }
            EnvelopeRejection::InvalidValue { field, detail } => {
                write!(f, "envelope field `{field}` is invalid: {detail}")
            }
            EnvelopeRejection::UnsupportedProtocolVersion(v) => {
                write!(
                    f,
                    "unsupported protocol version `{v}`; this build speaks {}",
                    vocab::PROTOCOL_VERSION
                )
            }
            EnvelopeRejection::MissingAuthorizationContext(name) => write!(
                f,
                "`{name}` authorizes a material action and requires `{}`",
                vocab::MATERIAL_REQUIRED_FIELDS.join("`, `")
            ),
            EnvelopeRejection::WrongType { field, expected } => {
                write!(f, "envelope field `{field}` must be {expected}")
            }
            EnvelopeRejection::WrongJsonType {
                field,
                got,
                allowed,
            } => write!(
                f,
                "envelope field `{field}` has JSON type {got}, but the contract permits {}",
                allowed.join(" or ")
            ),
            EnvelopeRejection::MissingNestedField { object, field } => {
                write!(f, "`{object}` is missing required field `{field}`")
            }
            EnvelopeRejection::UnknownNestedField { object, field } => {
                write!(f, "`{object}` carries undefined field `{field}`")
            }
            EnvelopeRejection::Malformed(detail) => {
                write!(f, "envelope text is not JSON: {detail}")
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
///
/// What it does carry is a typed reader per field the delivery bus needs. That is deliberate: the bus owns
/// transport and depends only on `protocol` and `storage`, so it cannot reach for a JSON library of its own.
/// Handing it the raw `Value` would make every bus field access a string lookup with an unwrap-shaped hole in
/// it, and the field names would live in the bus instead of beside the contract that defines them.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub value: Value,
}

impl Envelope {
    /// A required string field.
    ///
    /// Infallible because [`validate_envelope`] has already established that every required field is present
    /// with the contract's own JSON type, so the `unwrap_or_default` below is unreachable for any envelope this
    /// crate produced. It is not unreachable for one built by hand - `value` is public - which is why the bus
    /// accepts envelope *text* and validates it rather than accepting an `Envelope` a caller assembled.
    fn text(&self, field: &str) -> &str {
        self.value
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// An optional string field: absent and explicit `null` are both `None`.
    fn optional_text(&self, field: &str) -> Option<&str> {
        self.value.get(field).and_then(Value::as_str)
    }

    /// A required integer field.
    ///
    /// The same convention as [`Envelope::text`]: a field the contract requires is read infallibly, and a field
    /// it makes optional is read as an `Option`. `sequence` and `project_epoch` are the contract's only two
    /// integers and it requires both, so there is no optional-integer reader to pair with this one.
    fn integer(&self, field: &str) -> i64 {
        self.value
            .get(field)
            .and_then(Value::as_i64)
            .unwrap_or_default()
    }

    /// A boolean field, false when absent.
    fn flag(&self, field: &str) -> bool {
        self.value
            .get(field)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn message_type(&self) -> &str {
        self.text("message_type")
    }

    pub fn message_id(&self) -> &str {
        self.text("message_id")
    }

    pub fn event_id(&self) -> &str {
        self.text("event_id")
    }

    pub fn project_id(&self) -> &str {
        self.text("project_id")
    }

    pub fn session_id(&self) -> &str {
        self.text("session_id")
    }

    pub fn channel(&self) -> &str {
        self.text("channel")
    }

    pub fn phase(&self) -> &str {
        self.text("phase")
    }

    /// The sender's ordering position within `(session_id, channel)`.
    pub fn sequence(&self) -> i64 {
        self.integer("sequence")
    }

    /// The project epoch the sender believed current when it built this envelope.
    pub fn project_epoch(&self) -> i64 {
        self.integer("project_epoch")
    }

    pub fn correlation_id(&self) -> &str {
        self.text("correlation_id")
    }

    pub fn causation_id(&self) -> Option<&str> {
        self.optional_text("causation_id")
    }

    /// The key a retry of one logical request reuses, so the bus can recognise the retry.
    pub fn idempotency_key(&self) -> Option<&str> {
        self.optional_text("idempotency_key")
    }

    /// The operation this message performs. DEC-027 and the implementation design make `project_id +
    /// operation_id` the canonical idempotency scope, which is why the bus reads this and not the key alone.
    pub fn operation_id(&self) -> Option<&str> {
        self.optional_text("operation_id")
    }

    pub fn priority(&self) -> &str {
        self.text("priority")
    }

    pub fn created_at(&self) -> &str {
        self.text("created_at")
    }

    /// Whether the sender requires a receipt for this message.
    pub fn requires_ack(&self) -> bool {
        self.flag("requires_ack")
    }

    pub fn expires_at(&self) -> Option<&str> {
        self.optional_text("expires_at")
    }

    /// The envelope's `payload` member as compact JSON.
    ///
    /// The payload is the message's request body, and it is the part of an envelope that a retry must repeat
    /// exactly: a retry legitimately carries a new `message_id`, a new `sequence` and a new `created_at`, so
    /// comparing whole envelopes would call every retry a conflict and comparing nothing would call every
    /// conflict a retry. This is the text the bus compares.
    pub fn payload_json_text(&self) -> String {
        match self.value.get("payload") {
            Some(payload) => payload.to_string(),
            // Unreachable for a validated envelope, which must carry a `payload` object. `{}` rather than an
            // empty string because an empty string is not JSON, and a caller comparing it would be comparing
            // against a document that cannot exist.
            None => "{}".to_string(),
        }
    }

    /// The envelope as stored text: compact JSON with object members in sorted key order.
    ///
    /// Deterministic, so two envelopes that differ only in the sender's key order or whitespace produce the
    /// same stored text and the same bytes in `messages.envelope_json`. `serde_json`'s object map is a sorted
    /// map in this workspace (the `preserve_order` feature is not enabled anywhere), and a test in this crate
    /// pins that, because the property is load-bearing here and would otherwise be a feature flag away from
    /// being false.
    ///
    /// Not RFC 8785 JCS, and it does not need to be: JCS is required where the contract declares a digest over
    /// the value (DEC-025's `state_digest`, DEC-034's event chain). No digest is declared over an envelope, so
    /// re-encoding it as JCS would add a canonicalization step whose only effect would be to change bytes
    /// nothing compares.
    pub fn to_json_text(&self) -> String {
        self.value.to_string()
    }
}

/// Parse envelope text and validate it.
///
/// This is the entry point the delivery bus uses, because a bus that accepts an already-built [`Envelope`]
/// would be trusting a value it did not check: `Envelope::value` is public, so one can be assembled by hand
/// with any shape at all. Text in, validated envelope out, leaves no such gap.
pub fn parse_envelope(text: &str) -> Result<Envelope, EnvelopeRejection> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| EnvelopeRejection::Malformed(e.to_string()))?;
    validate_envelope(&value)
}

/// The name this validator reports for a value's JSON type.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Does `value` hold the JSON type the generated table names?
///
/// The names come from the contract's `type` keyword in the generator. `Int` admits a float with no fractional
/// part, because JSON Schema's `integer` is a mathematical test rather than a lexical one; a stricter reading
/// here would reject a document the contract admits, which is the same defect from the other direction.
fn holds_type(value: &Value, kind: &str) -> bool {
    match kind {
        "Null" => value.is_null(),
        "Bool" => value.is_boolean(),
        "Str" => value.is_string(),
        "Arr" => value.is_array(),
        "Obj" => value.is_object(),
        "Num" => value.is_number(),
        "Int" => {
            value.as_i64().is_some()
                || value.as_u64().is_some()
                || value.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        _ => false,
    }
}

/// Validate a JSON value as an MCF-v2 envelope.
///
/// Rejects, in order: a non-object; a wrong protocol version; a missing required field; an undefined field; a
/// field whose JSON type the contract does not permit; a field outside its vocabulary; a malformed
/// `schema_version`, sequence, epoch or digest; a malformed identity or recipient list; a malformed security
/// block; and a material-action message that omits or nulls any field its conditional requires. Every rejection
/// is specific - a caller is never told merely "invalid".
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
        if !vocab::REQUIRED_FIELDS.contains(&key.as_str())
            && !vocab::OPTIONAL_FIELDS.contains(&key.as_str())
        {
            return Err(EnvelopeRejection::UnknownField(key.clone()));
        }
    }

    // Types from the contract's own `type` keyword, never hand-copied. A field whose contract declares no
    // single type (a `$ref`, a bare `enum`, a bare `const`) is absent from this table by construction and is
    // checked by the rule named for it below.
    for &(field, allowed) in vocab::FIELD_TYPES {
        if let Some(held) = obj.get(field) {
            if !allowed.iter().any(|kind| holds_type(held, kind)) {
                return Err(EnvelopeRejection::WrongJsonType {
                    field,
                    got: json_type_name(held),
                    allowed,
                });
            }
        }
    }

    check_enum(obj, "channel", vocab::CHANNELS)?;
    check_enum(obj, "phase", vocab::PHASES)?;
    check_enum(obj, "message_type", vocab::MESSAGE_TYPES)?;
    check_enum(obj, "priority", vocab::PRIORITIES)?;
    check_schema_version(obj)?;
    check_minimum(obj, "sequence")?;
    check_minimum(obj, "project_epoch")?;
    check_non_empty_string(obj, "idempotency_key")?;

    let sender = obj
        .get("sender")
        .ok_or(EnvelopeRejection::MissingField("sender"))?;
    check_identity(sender, "sender")?;
    check_recipients(obj)?;
    check_state_digest(obj)?;
    check_security(obj)?;

    // The conditional rule: a material action must arrive with the authorization context that permits it. The
    // contract's `then` narrows every one of these fields to a non-nullable type, so presence alone is not
    // enough - a field explicitly set to null does not satisfy it.
    let message_type = obj
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if vocab::MATERIAL_ACTION_MESSAGE_TYPES.contains(&message_type) {
        for field in vocab::MATERIAL_REQUIRED_FIELDS {
            let present = obj.get(*field).map(|v| !v.is_null()).unwrap_or(false);
            if !present {
                return Err(EnvelopeRejection::MissingAuthorizationContext(
                    message_type.to_string(),
                ));
            }
        }
        check_authorization_context(obj)?;
    }

    Ok(Envelope {
        value: value.clone(),
    })
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
        Some(_) => Err(EnvelopeRejection::WrongType {
            field,
            expected: "a string",
        }),
        None => Err(EnvelopeRejection::MissingField(field)),
    }
}

/// A nested object's `required` and `additionalProperties:false`, both read from the contract.
///
/// This is the check whose absence let an `authorization_context` through without the `required_capabilities`
/// the contract demands: the field list was hand-copied into the validator, and it had drifted by one.
fn check_nested_shape(
    map: &serde_json::Map<String, Value>,
    object: &'static str,
    required: &[&'static str],
    allowed: &[&'static str],
) -> Result<(), EnvelopeRejection> {
    for field in required {
        if !map.contains_key(*field) {
            return Err(EnvelopeRejection::MissingNestedField { object, field });
        }
    }
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(EnvelopeRejection::UnknownNestedField {
                object,
                field: key.clone(),
            });
        }
    }
    Ok(())
}

fn check_nested_enum(
    map: &serde_json::Map<String, Value>,
    object: &'static str,
    field: &'static str,
    allowed: &[&str],
) -> Result<(), EnvelopeRejection> {
    match map.get(field) {
        Some(Value::String(s)) if allowed.contains(&s.as_str()) => Ok(()),
        Some(Value::String(s)) => Err(EnvelopeRejection::InvalidValue {
            field,
            detail: format!(
                "`{s}` is not one of the {} legal values for `{object}`",
                allowed.len()
            ),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType {
            field,
            expected: "a string",
        }),
        None => Err(EnvelopeRejection::MissingNestedField { object, field }),
    }
}

fn check_schema_version(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    let raw = match obj.get("schema_version") {
        Some(Value::String(v)) => v.clone(),
        Some(_) => {
            return Err(EnvelopeRejection::WrongType {
                field: "schema_version",
                expected: "a string",
            })
        }
        None => return Err(EnvelopeRejection::MissingField("schema_version")),
    };

    // The contract's pattern is `^<major>\.\d+\.\d+$`: the MAJOR is pinned and MINOR and PATCH must be
    // non-empty digits. The pinned MAJOR is generated from that same pattern, so this function cannot drift
    // from it without the generator refusing to emit.
    let parts: Vec<&str> = raw.split('.').collect();
    let all_numeric = parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if parts.len() == 3 && all_numeric && parts[0] == vocab::SCHEMA_VERSION_MAJOR {
        Ok(())
    } else {
        Err(EnvelopeRejection::InvalidValue {
            field: "schema_version",
            detail: format!(
                "`{raw}` must be {}.<numeric minor>.<numeric patch>",
                vocab::SCHEMA_VERSION_MAJOR
            ),
        })
    }
}

/// A `minimum: 0` integer, which the type table cannot express.
fn check_minimum(
    obj: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<(), EnvelopeRejection> {
    match obj.get(field) {
        Some(Value::Number(n)) if n.as_i64().is_some_and(|v| v >= 0) => Ok(()),
        Some(Value::Number(_)) => Err(EnvelopeRejection::InvalidValue {
            field,
            detail: "must be zero or greater".to_string(),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType {
            field,
            expected: "an integer",
        }),
        None => Err(EnvelopeRejection::MissingField(field)),
    }
}

/// A nullable field the contract gives `minLength: 1`. Absent and null are both legal; an empty string is not.
fn check_non_empty_string(
    obj: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<(), EnvelopeRejection> {
    match obj.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(s)) if !s.is_empty() => Ok(()),
        Some(Value::String(_)) => Err(EnvelopeRejection::InvalidValue {
            field,
            detail: "must not be empty when present".to_string(),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType {
            field,
            expected: "a string or null",
        }),
    }
}

/// `uniqueItems: true` on an array of strings, which the type table cannot express.
fn check_unique_strings(value: &Value, field: &'static str) -> Result<(), EnvelopeRejection> {
    let items = value.as_array().ok_or(EnvelopeRejection::WrongType {
        field,
        expected: "an array of strings",
    })?;

    let mut seen: Vec<&str> = Vec::with_capacity(items.len());
    for item in items {
        let s = item.as_str().ok_or(EnvelopeRejection::WrongType {
            field,
            expected: "an array of strings",
        })?;
        if seen.contains(&s) {
            return Err(EnvelopeRejection::InvalidValue {
                field,
                detail: format!("repeats `{s}`"),
            });
        }
        seen.push(s);
    }
    Ok(())
}

/// An actor identity: the contract's own `identity.schema.json`, applied to the sender and to every recipient
/// alike. The same rule must hold in both positions - a recipient must not be a shape the sender may not be.
fn check_identity(identity: &Value, object: &'static str) -> Result<(), EnvelopeRejection> {
    let map = identity.as_object().ok_or(EnvelopeRejection::WrongType {
        field: object,
        expected: "an identity object",
    })?;

    check_nested_shape(
        map,
        object,
        vocab::IDENTITY_REQUIRED_FIELDS,
        vocab::IDENTITY_FIELDS,
    )?;
    check_nested_enum(map, object, "actor_type", vocab::ACTOR_TYPES)?;

    // actor_id carries minLength:1 - an identity with no id names nobody.
    match map.get("actor_id") {
        Some(Value::String(s)) if !s.is_empty() => {}
        Some(Value::String(_)) => {
            return Err(EnvelopeRejection::InvalidValue {
                field: "actor_id",
                detail: "must not be empty".into(),
            })
        }
        _ => {
            return Err(EnvelopeRejection::WrongType {
                field: "actor_id",
                expected: "a non-empty string",
            })
        }
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
            _ => {
                return Err(EnvelopeRejection::WrongType {
                    field: "agent_type",
                    expected: "a string or null",
                })
            }
        }
    }
    Ok(())
}

fn check_recipients(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    let recipients = obj
        .get("recipients")
        .ok_or(EnvelopeRejection::MissingField("recipients"))?;
    let list = recipients.as_array().ok_or(EnvelopeRejection::WrongType {
        field: "recipients",
        expected: "an array",
    })?;

    // minItems:1 in the contract. An envelope addressed to nobody is a routing defect, not a broadcast.
    if list.is_empty() {
        return Err(EnvelopeRejection::InvalidValue {
            field: "recipients",
            detail: "must name at least one recipient".into(),
        });
    }
    // Each recipient is a full identity, not merely a typed actor: the contract requires actor_id on every one.
    for recipient in list {
        check_identity(recipient, "recipients[]")?;
    }
    Ok(())
}

/// A state digest is 64 hex characters - a SHA-256 over JCS-canonical input (DEC-025). A malformed digest
/// would defeat staleness detection rather than merely look untidy.
fn check_state_digest(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    match obj.get("state_digest") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(s)) if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) => {
            Ok(())
        }
        Some(Value::String(s)) => Err(EnvelopeRejection::InvalidValue {
            field: "state_digest",
            detail: format!("`{s}` is not 64 hex characters"),
        }),
        Some(_) => Err(EnvelopeRejection::WrongType {
            field: "state_digest",
            expected: "a string or null",
        }),
    }
}

fn check_security(obj: &serde_json::Map<String, Value>) -> Result<(), EnvelopeRejection> {
    let security = obj
        .get("security")
        .ok_or(EnvelopeRejection::MissingField("security"))?;
    let map = security.as_object().ok_or(EnvelopeRejection::WrongType {
        field: "security",
        expected: "an object",
    })?;

    check_nested_shape(
        map,
        "security",
        vocab::SECURITY_REQUIRED_FIELDS,
        vocab::SECURITY_FIELDS,
    )?;
    check_nested_enum(map, "security", "classification", vocab::CLASSIFICATIONS)?;

    // secret_refs is required, and holds unique strings: secrets are referenced, never carried.
    let refs = map
        .get("secret_refs")
        .ok_or(EnvelopeRejection::MissingNestedField {
            object: "security",
            field: "secret_refs",
        })?;
    check_unique_strings(refs, "secret_refs")?;

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

fn check_authorization_context(
    obj: &serde_json::Map<String, Value>,
) -> Result<(), EnvelopeRejection> {
    let context = obj
        .get("authorization_context")
        .ok_or(EnvelopeRejection::MissingField("authorization_context"))?;
    let map = context.as_object().ok_or(EnvelopeRejection::WrongType {
        field: "authorization_context",
        expected: "an object",
    })?;

    // All seven fields, including required_capabilities - which PolicyService consumes and a hand-copied list
    // had silently stopped requiring.
    check_nested_shape(
        map,
        "authorization_context",
        vocab::AUTHORIZATION_CONTEXT_REQUIRED_FIELDS,
        vocab::AUTHORIZATION_CONTEXT_FIELDS,
    )?;

    for field in [
        "lease_id",
        "workspace_id",
        "agent_id",
        "policy_scope",
        "capability_snapshot_id",
    ] {
        match map.get(field) {
            Some(Value::String(s)) if !s.is_empty() => {}
            Some(Value::String(_)) => {
                return Err(EnvelopeRejection::InvalidValue {
                    field,
                    detail: "must not be empty".into(),
                })
            }
            _ => {
                return Err(EnvelopeRejection::WrongType {
                    field,
                    expected: "a non-empty string",
                })
            }
        }
    }

    // lease_version is 1-based in the contract; a zero or negative lease version is not a lease.
    match map.get("lease_version") {
        Some(Value::Number(n)) if n.as_i64().is_some_and(|v| v >= 1) => {}
        _ => {
            return Err(EnvelopeRejection::InvalidValue {
                field: "lease_version",
                detail: "must be an integer of 1 or more".into(),
            })
        }
    }

    let capabilities =
        map.get("required_capabilities")
            .ok_or(EnvelopeRejection::MissingNestedField {
                object: "authorization_context",
                field: "required_capabilities",
            })?;
    check_unique_strings(capabilities, "required_capabilities")
}
