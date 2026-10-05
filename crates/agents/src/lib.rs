//! Controller-owned agent gateway primitives.
//!
//! Process creation and cancellation remain in \`mayasaba-execution\`. This crate owns the closed agent
//! identity set, contract-driven launch preparation, admission proofs and native-event normalization.
//! The native transport contract is embedded from its canonical source so launch vectors and safety controls
//! are not copied into a second machine-readable authority.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const NATIVE_TRANSPORT_CONTRACT: &str =
    include_str!("../../../schemas/agent-adapter-v1/native-transport-contract.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    #[serde(rename = "HERMES_AGENT")]
    Hermes,
    #[serde(rename = "KILO_CODE")]
    Kilo,
    #[serde(rename = "OPEN_CODE")]
    OpenCode,
}

impl AgentType {
    pub const ALL: [Self; 3] = [Self::Hermes, Self::Kilo, Self::OpenCode];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hermes => "HERMES_AGENT",
            Self::Kilo => "KILO_CODE",
            Self::OpenCode => "OPEN_CODE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "HERMES_AGENT" => Some(Self::Hermes),
            "KILO_CODE" => Some(Self::Kilo),
            "OPEN_CODE" => Some(Self::OpenCode),
            _ => None,
        }
    }

    pub const fn lineage_group(self) -> &'static str {
        match self {
            Self::Hermes => "HERMES",
            Self::Kilo | Self::OpenCode => "OPENCODE_FORK",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    #[serde(rename = "HERMES_STREAM_JSON")]
    HermesStreamJson,
    #[serde(rename = "KILO_JSON")]
    KiloJson,
    #[serde(rename = "OPEN_CODE_JSON")]
    OpenCodeJson,
    #[serde(rename = "ACP_STDIO")]
    AcpStdio,
    #[serde(rename = "UNSUPPORTED")]
    Unsupported,
}

impl Transport {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "HERMES_STREAM_JSON" => Some(Self::HermesStreamJson),
            "KILO_JSON" => Some(Self::KiloJson),
            "OPEN_CODE_JSON" => Some(Self::OpenCodeJson),
            "ACP_STDIO" => Some(Self::AcpStdio),
            "UNSUPPORTED" => Some(Self::Unsupported),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentVersion {
    pub raw: String,
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    #[serde(default)]
    pub prerelease: Option<String>,
}

impl AgentVersion {
    pub fn parse(raw: impl Into<String>) -> Option<Self> {
        let raw = raw.into();
        let trimmed = raw.trim().trim_start_matches('v');
        let core = trimmed.split_once('+').map_or(trimmed, |(v, _)| v);
        let (core, prerelease) = core
            .split_once('-')
            .map_or((core, None), |(v, suffix)| (v, Some(suffix.to_owned())));
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self { raw, major, minor, patch, prerelease })
    }

    pub const fn line(self) -> &'static str {
        match self.major {
            0 => "0.x",
            1 => "1.x",
            2 => "2.x",
            _ => "unknown",
        }
    }
}

pub type CapabilitySet = BTreeMap<String, bool>;
pub type Environment = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInstallation {
    pub agent_type: AgentType,
    pub executable: String,
    pub resolved_path: String,
    pub version: AgentVersion,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    pub state: HealthState,
    pub checked_at: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthState {
    HEALTHY,
    DEGRADED,
    UNHEALTHY,
    UNKNOWN,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLaunch {
    pub project_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub transport: Transport,
    pub prompt_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionHandle {
    pub process_id: u32,
    pub native_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendReceipt {
    pub message_id: String,
    pub accepted: bool,
    pub received_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeEventStream {
    pub transport: String,
    pub protocol_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    USER_STOP,
    TASK_CANCEL,
    PROJECT_STOP,
    TIMEOUT,
    POLICY,
    FAILURE,
    RECOVERY,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeScope {
    pub workspace_id: String,
    pub allowed_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub files: Vec<Value>,
    pub diff_ref: Option<String>,
    pub checkpoint_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceScope {
    pub task_id: String,
    pub workspace_id: String,
    pub kinds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRefs {
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterError {
    pub category: AdapterErrorCategory,
    pub code: String,
    pub retryability: Retryability,
    pub message: String,
    pub causation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterErrorCategory {
    DETECTION,
    LAUNCH,
    TRANSPORT,
    PARSE,
    PROCESS,
    TIMEOUT,
    CANCELLATION,
    AUTHENTICATION,
    CAPABILITY,
    WORKSPACE,
    PROTOCOL,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Retryability {
    NEVER,
    IMMEDIATE,
    BACKOFF,
    AFTER_SYNC,
    AFTER_REASSIGNMENT,
    AFTER_USER_ACTION,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeEventKind {
    SESSION_STARTED,
    TEXT,
    REASONING,
    STEP_START,
    STEP_FINISH,
    TOOL_CALL,
    TOOL_RESULT,
    FILE_CHANGE,
    COMMAND_REQUEST,
    COMMAND_RESULT,
    ERROR,
    RESULT,
    SESSION_ENDED,
    HEARTBEAT,
    UNKNOWN,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeEvent {
    pub agent_type: AgentType,
    pub transport: Transport,
    pub native_kind: NativeEventKind,
    pub native_session_id: Option<String>,
    pub received_at: String,
    pub raw_ref: String,
    pub payload: Value,
}

/// A controller-produced proof. Missing proof is a hard launch refusal; this is not a defaulting mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchProof {
    pub workspace_validated: bool,
    pub policy_validated: bool,
    pub capability_validated: bool,
    pub transport_validated: bool,
    pub native_configuration_validated: bool,
}

impl LaunchProof {
    pub const fn complete(self) -> bool {
        self.workspace_validated
            && self.policy_validated
            && self.capability_validated
            && self.transport_validated
            && self.native_configuration_validated
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedLaunch {
    pub agent_type: AgentType,
    pub executable: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub environment: Environment,
    /// Variables the execution kernel must remove from the inherited child environment before spawn.
    pub scrub_inherited_environment: Vec<String>,
    /// Effective vendor config document to inject through the contract-declared channel.
    pub required_config: Value,
    pub transport: Transport,
    pub resume: bool,
}

pub trait AgentAdapter {
    fn agent_type(&self) -> AgentType;

    fn prepare_launch(
        &self,
        request: &SessionLaunch,
        version: &AgentVersion,
        prompt: &str,
        native_session_id: Option<&str>,
        proof: LaunchProof,
    ) -> Result<PreparedLaunch, AdapterError>;

    fn normalize_event(
        &self,
        raw: &str,
        received_at: &str,
        raw_ref: &str,
    ) -> Result<NativeEvent, AdapterError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HermesAdapter;
#[derive(Debug, Clone, Copy, Default)]
pub struct KiloAdapter;
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenCodeAdapter;

impl PreparedLaunch {
    /// Convert the adapter-owned launch description into the process-neutral execution specification.
    /// Execution remains the sole owner of actual spawn/termination.
    pub fn process_spec(&self) -> mayasaba_execution::ProcessSpec {
        mayasaba_execution::ProcessSpec {
            executable: self.executable.clone(),
            argv: self.argv.clone(),
            cwd: self.cwd.clone(),
            environment: self.environment.clone(),
            scrub_inherited_environment: self.scrub_inherited_environment.clone(),
        }
    }
}

impl AgentAdapter for HermesAdapter {
    fn agent_type(&self) -> AgentType { AgentType::Hermes }
    fn prepare_launch(&self, request: &SessionLaunch, version: &AgentVersion, prompt: &str,
        native_session_id: Option<&str>, proof: LaunchProof) -> Result<PreparedLaunch, AdapterError> {
        prepare_from_contract(AgentType::Hermes, request, version, prompt, native_session_id, proof)
    }
    fn normalize_event(&self, raw: &str, received_at: &str, raw_ref: &str)
        -> Result<NativeEvent, AdapterError> {
        normalize_native_event(AgentType::Hermes, raw, received_at, raw_ref)
    }
}

impl AgentAdapter for KiloAdapter {
    fn agent_type(&self) -> AgentType { AgentType::Kilo }
    fn prepare_launch(&self, request: &SessionLaunch, version: &AgentVersion, prompt: &str,
        native_session_id: Option<&str>, proof: LaunchProof) -> Result<PreparedLaunch, AdapterError> {
        prepare_from_contract(AgentType::Kilo, request, version, prompt, native_session_id, proof)
    }
    fn normalize_event(&self, raw: &str, received_at: &str, raw_ref: &str)
        -> Result<NativeEvent, AdapterError> {
        normalize_native_event(AgentType::Kilo, raw, received_at, raw_ref)
    }
}

impl AgentAdapter for OpenCodeAdapter {
    fn agent_type(&self) -> AgentType { AgentType::OpenCode }
    fn prepare_launch(&self, request: &SessionLaunch, version: &AgentVersion, prompt: &str,
        native_session_id: Option<&str>, proof: LaunchProof) -> Result<PreparedLaunch, AdapterError> {
        prepare_from_contract(AgentType::OpenCode, request, version, prompt, native_session_id, proof)
    }
    fn normalize_event(&self, raw: &str, received_at: &str, raw_ref: &str)
        -> Result<NativeEvent, AdapterError> {
        normalize_native_event(AgentType::OpenCode, raw, received_at, raw_ref)
    }
}

fn adapter_error(
    category: AdapterErrorCategory,
    code: &'static str,
    retryability: Retryability,
    message: impl Into<String>,
) -> AdapterError {
    AdapterError { category, code: code.to_owned(), retryability, message: message.into(), causation_id: None }
}

fn contract() -> Result<Value, AdapterError> {
    serde_json::from_str(NATIVE_TRANSPORT_CONTRACT).map_err(|e| adapter_error(
        AdapterErrorCategory::PROTOCOL, "ADAPTER_CONTRACT_INVALID", Retryability::NEVER,
        format!("native transport contract is invalid JSON: {e}")))
}

fn agent_contract(agent: AgentType) -> Result<Value, AdapterError> {
    contract()?
        .get("agents").and_then(|v| v.get(agent.as_str())).cloned()
        .ok_or_else(|| adapter_error(
            AdapterErrorCategory::PROTOCOL, "ADAPTER_CONTRACT_MISSING_AGENT", Retryability::NEVER,
            format!("native transport contract has no {}", agent.as_str())))
}

fn expand_vector(
    definition: &Value,
    resume: bool,
    prompt: &str,
    workspace: &str,
    native_session_id: Option<&str>,
) -> Result<Vec<String>, AdapterError> {
    let key = if resume { "resume" } else { "launch" };
    let vector = definition.get(key).and_then(Value::as_array).ok_or_else(|| adapter_error(
        AdapterErrorCategory::PROTOCOL, "ADAPTER_LAUNCH_VECTOR_MISSING", Retryability::NEVER,
        format!("native transport contract has no {key} vector")))?;
    vector.iter().map(|token| {
        let token = token.as_str().ok_or_else(|| adapter_error(
            AdapterErrorCategory::PROTOCOL, "ADAPTER_LAUNCH_TOKEN_INVALID", Retryability::NEVER,
            "launch vector contains a non-string token"))?;
        match token {
            "<prompt>" => Ok(prompt.to_owned()),
            "<workspace>" => Ok(workspace.to_owned()),
            "<native_session_id>" => native_session_id.map(ToOwned::to_owned).ok_or_else(|| adapter_error(
                AdapterErrorCategory::LAUNCH, "NATIVE_SESSION_REQUIRED", Retryability::AFTER_SYNC,
                "resume requested but native session identity is absent")),
            value => Ok(value.to_owned()),
        }
    }).collect()
}

fn prepare_from_contract(
    agent: AgentType,
    request: &SessionLaunch,
    version: &AgentVersion,
    prompt: &str,
    native_session_id: Option<&str>,
    proof: LaunchProof,
) -> Result<PreparedLaunch, AdapterError> {
    if !proof.complete() {
        return Err(adapter_error(
            AdapterErrorCategory::LAUNCH, "LAUNCH_GATE_INCOMPLETE", Retryability::AFTER_SYNC,
            "launch requires workspace, policy, capability, transport and native-configuration proofs"));
    }
    if !is_absolute_windows_path(&request.cwd) {
        return Err(adapter_error(
            AdapterErrorCategory::WORKSPACE, "WORKSPACE_PATH_NOT_ABSOLUTE", Retryability::AFTER_USER_ACTION,
            "agent launch cwd must be an absolute Windows path"));
    }
    if request.prompt_ref.is_none() && prompt.trim().is_empty() {
        return Err(adapter_error(
            AdapterErrorCategory::LAUNCH, "PROMPT_EMPTY", Retryability::NEVER,
            "agent launch requires a non-empty prompt or prompt_ref"));
    }

    let definition = agent_contract(agent)?;
    if let Some(lines) = definition
        .get("version_gate").and_then(|v| v.get("admitted_lines")).and_then(Value::as_array)
    {
        if !lines.iter().any(|v| v.as_str() == Some(version.line())) {
            return Err(adapter_error(
                AdapterErrorCategory::CAPABILITY, "AGENT_VERSION_UNADMITTED", Retryability::AFTER_USER_ACTION,
                format!("{} version {} is not an admitted native line", agent.as_str(), version.raw)));
        }
    }

    let resume = native_session_id.is_some();
    let mut argv = expand_vector(&definition, resume, prompt, &request.cwd, native_session_id)?;

    if let Some(flags) = definition.get("isolation_flags").and_then(Value::as_array) {
        argv.extend(flags.iter().filter_map(Value::as_str).map(ToOwned::to_owned));
    }

    let mut environment = Environment::new();
    if let Some(required) = definition.get("required_environment").and_then(Value::as_object) {
        for (key, value) in required {
            if let Some(value) = value.as_str() {
                environment.insert(key.clone(), value.to_owned());
            }
        }
    }

    let mut required_config = definition.get("required_config").cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    // required_config_injection is contract metadata. Only its actual config-bearing fields belong in the
    // vendor config document: the permission map is inserted as "permission", while audit rationale and probe
    // instructions remain controller metadata and must never be handed to the CLI as unknown config keys.
    if let Some(injection) = definition.get("required_config_injection") {
        let channel = injection.get("channel").and_then(Value::as_str).ok_or_else(|| adapter_error(
            AdapterErrorCategory::PROTOCOL, "CONFIG_INJECTION_CHANNEL_MISSING", Retryability::NEVER,
            "required_config_injection has no channel"))?;
        if let Some(object) = required_config.as_object_mut() {
            if let Some(permission) = injection.get("permission") {
                object.insert("permission".to_owned(), permission.clone());
            }
            if let Some(must_include) = injection.get("must_include").and_then(Value::as_object) {
                for (key, value) in must_include {
                    object.insert(key.clone(), value.clone());
                }
            }
        }
        let encoded = serde_json::to_string(&required_config).map_err(|e| adapter_error(
            AdapterErrorCategory::PROTOCOL, "CONFIG_INJECTION_SERIALIZE_FAILED", Retryability::NEVER,
            format!("cannot serialize effective config document: {e}")))?;
        environment.insert(channel.to_owned(), encoded);
    }

    // The inherited parent environment is outside Mayasaba authority. The execution kernel must scrub these
    // keys before spawn; setting replacement values alone is insufficient when a stale inherited value activates
    // an approval bypass or wrong workspace.
    let mut scrub_inherited_environment = Vec::new();
    if agent == AgentType::Hermes {
        scrub_inherited_environment.extend([
            "HERMES_YOLO_MODE".to_owned(),
            "HERMES_ACCEPT_HOOKS".to_owned(),
        ]);
    }
    // Kilo and OpenCode resolve workspace from PWD on the relevant lineage; explicitly bind it to the authorized
    // workspace rather than inheriting a stale shell value (especially from MSYS/Git Bash on Windows).
    scrub_inherited_environment.push("PWD".to_owned());
    environment.insert("PWD".to_owned(), request.cwd.clone());

    let transport_text = definition.get("transport").and_then(Value::as_str).ok_or_else(|| adapter_error(
        AdapterErrorCategory::PROTOCOL, "ADAPTER_TRANSPORT_MISSING", Retryability::NEVER,
        "native transport contract has no transport"))?;
    let transport = Transport::parse(transport_text).ok_or_else(|| adapter_error(
        AdapterErrorCategory::PROTOCOL, "ADAPTER_TRANSPORT_UNKNOWN", Retryability::NEVER,
        format!("unknown transport {transport_text}")))?;
    if transport != request.transport {
        return Err(adapter_error(
            AdapterErrorCategory::TRANSPORT, "TRANSPORT_MISMATCH", Retryability::AFTER_SYNC,
            format!("request transport {:?} does not match contract transport {:?}", request.transport, transport)));
    }

    let required_config = definition.get("required_config").cloned().unwrap_or_else(|| Value::Object(Default::default()));
    Ok(PreparedLaunch {
        agent_type: agent,
        executable: definition.get("executable").and_then(Value::as_str).unwrap_or_default().to_owned(),
        argv,
        cwd: request.cwd.clone(),
        environment,
        scrub_inherited_environment,
        required_config,
        transport,
        resume,
    })
}

fn normalize_native_event(
    agent: AgentType,
    raw: &str,
    received_at: &str,
    raw_ref: &str,
) -> Result<NativeEvent, AdapterError> {
    if received_at.trim().is_empty() || raw_ref.trim().is_empty() {
        return Err(adapter_error(
            AdapterErrorCategory::PARSE, "EVENT_PROVENANCE_MISSING", Retryability::NEVER,
            "normalized native events require received_at and raw_ref"));
    }
    let payload: Value = serde_json::from_str(raw).map_err(|e| adapter_error(
        AdapterErrorCategory::PARSE, "NATIVE_EVENT_NOT_JSON", Retryability::NEVER,
        format!("native event is not JSON: {e}")))?;
    let ty = payload.get("type").and_then(Value::as_str).unwrap_or_default();

    let kind = match agent {
        AgentType::Hermes => match ty {
            "text" => NativeEventKind::TEXT,
            "tool_use" => NativeEventKind::TOOL_CALL,
            "tool_result" => NativeEventKind::TOOL_RESULT,
            "result" => NativeEventKind::RESULT,
            "error" => NativeEventKind::ERROR,
            "heartbeat" => NativeEventKind::HEARTBEAT,
            _ => NativeEventKind::UNKNOWN,
        },
        AgentType::Kilo | AgentType::OpenCode => match ty {
            "step_start" | "step-start" => NativeEventKind::STEP_START,
            "step_finish" | "step-finish" => NativeEventKind::STEP_FINISH,
            "text" => NativeEventKind::TEXT,
            "reasoning" => NativeEventKind::REASONING,
            "tool_use" => NativeEventKind::TOOL_CALL,
            "tool_result" => NativeEventKind::TOOL_RESULT,
            "error" => NativeEventKind::ERROR,
            "heartbeat" => NativeEventKind::HEARTBEAT,
            _ => NativeEventKind::UNKNOWN,
        },
    };

    let native_session_id = payload.get("session_id").and_then(Value::as_str)
        .or_else(|| payload.get("sessionID").and_then(Value::as_str))
        .or_else(|| payload.get("sessionId").and_then(Value::as_str))
        .map(ToOwned::to_owned);
    let transport = match agent {
        AgentType::Hermes => Transport::HermesStreamJson,
        AgentType::Kilo => Transport::KiloJson,
        AgentType::OpenCode => Transport::OpenCodeJson,
    };

    Ok(NativeEvent {
        agent_type: agent,
        transport,
        native_kind: kind,
        native_session_id,
        received_at: received_at.to_owned(),
        raw_ref: raw_ref.to_owned(),
        payload,
    })
}

fn is_absolute_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\\\' || bytes[2] == b'/'))
        || path.starts_with("\\\\\\\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof() -> LaunchProof {
        LaunchProof {
            workspace_validated: true,
            policy_validated: true,
            capability_validated: true,
            transport_validated: true,
            native_configuration_validated: true,
        }
    }

    fn request(transport: Transport) -> SessionLaunch {
        SessionLaunch {
            project_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            session_id: "22222222-2222-4222-8222-222222222222".to_owned(),
            workspace_id: "33333333-3333-4333-8333-333333333333".to_owned(),
            cwd: r"C:\Work\Mayasaba".to_owned(),
            transport,
            prompt_ref: None,
        }
    }

    #[test]
    fn closed_agent_set_is_exactly_three() {
        assert_eq!(AgentType::ALL.map(AgentType::as_str),
            ["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"]);
        assert!(AgentType::parse("CLAUDE_CODE").is_none());
        assert!(AgentType::parse("CLINE").is_none());
    }

    #[test]
    fn version_parser_extracts_line() {
        let version = AgentVersion::parse("v2.0.22").expect("valid version");
        assert_eq!((version.major, version.minor, version.patch), (2, 0, 22));
        assert_eq!(version.line(), "2.x");
        assert_eq!(AgentVersion::parse("0.21.5").unwrap().line(), "0.x");
    }

    #[test]
    fn open_code_2x_is_refused_by_contract() {
        let error = OpenCodeAdapter.prepare_launch(
            &request(Transport::OpenCodeJson),
            &AgentVersion::parse("2.0.22").unwrap(),
            "work",
            None,
            proof(),
        ).expect_err("the current contract admits only OpenCode 1.x");
        assert_eq!(error.code, "AGENT_VERSION_UNADMITTED");
    }

    #[test]
    fn hermes_launch_uses_canonical_contract() {
        let prepared = HermesAdapter.prepare_launch(
            &request(Transport::HermesStreamJson),
            &AgentVersion::parse("0.21.5").unwrap(),
            "work",
            None,
            proof(),
        ).expect("prepared");
        assert_eq!(prepared.executable, "hermes");
        assert!(prepared.argv.contains(&"--ignore-rules".to_owned()));
    }

    #[test]
    fn kilo_launch_carries_required_local_restrictions() {
        let prepared = KiloAdapter.prepare_launch(
            &request(Transport::KiloJson),
            &AgentVersion::parse("7.8.1").unwrap(),
            "work",
            None,
            proof(),
        ).expect("prepared");
        assert_eq!(prepared.executable, "kilo");
        assert_eq!(prepared.environment.get("KILO_DISABLE_SHARE"), Some(&"1".to_owned()));
        assert!(prepared.environment.contains_key("KILO_CONFIG_CONTENT"));
        assert!(prepared.scrub_inherited_environment.contains(&"PWD".to_owned()));
        assert_eq!(prepared.required_config.get("share").and_then(Value::as_str), Some("disabled"));
        assert!(prepared.required_config.get("permission").is_some());
        assert!(prepared.required_config.get("rationale").is_none());
    }

    #[test]
    fn incomplete_proof_is_fail_closed() {
        let bad = LaunchProof { transport_validated: false, ..proof() };
        let error = HermesAdapter.prepare_launch(
            &request(Transport::HermesStreamJson),
            &AgentVersion::parse("0.21.5").unwrap(),
            "work",
            None,
            bad,
        ).expect_err("missing gate proof must refuse");
        assert_eq!(error.code, "LAUNCH_GATE_INCOMPLETE");
    }

    #[test]
    fn normalization_preserves_unknown_records() {
        let event = OpenCodeAdapter.normalize_event(
            r#"{"type":"step_start","sessionID":"native-1","part":{}}"#,
            "2026-10-05T15:30:00Z",
            "artifact://raw/1",
        ).expect("json");
        assert_eq!(event.native_kind, NativeEventKind::STEP_START);
        assert_eq!(event.native_session_id.as_deref(), Some("native-1"));

        let unknown = OpenCodeAdapter.normalize_event(
            r#"{"type":"future","sessionID":"native-1"}"#,
            "2026-10-05T15:30:01Z",
            "artifact://raw/2",
        ).expect("unknown native record");
        assert_eq!(unknown.native_kind, NativeEventKind::UNKNOWN);
    }
}
