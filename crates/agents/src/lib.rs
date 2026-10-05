//! Controller-owned agent gateway primitives.
//!
//! Process creation and cancellation remain in \`mayasaba-execution\`. This crate owns the closed agent
//! identity set, contract-driven launch preparation, admission proofs and native-event normalization.
//! The native transport contract is embedded from its canonical source so launch vectors and safety controls
//! are not copied into a second machine-readable authority.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::{process::Command, time::{self, Duration}};

const NATIVE_TRANSPORT_CONTRACT: &str =
const NATIVE_TO_MCF_REGISTRY: &str = include_str!("../../../schemas/agent-adapter-v1/native-to-mcf.registry.json");
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
    
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HermesStreamJson => "HERMES_STREAM_JSON",
            Self::KiloJson => "KILO_JSON",
            Self::OpenCodeJson => "OPEN_CODE_JSON",
            Self::AcpStdio => "ACP_STDIO",
            Self::Unsupported => "UNSUPPORTED",
        }
    }
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
pub struct ProbeOptions {
    pub cwd: String,
    pub probe_time: String,
    pub timeout_ms: u64,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().ok()
                .and_then(|p| p.to_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| r"C:\".to_owned()),
            probe_time: String::new(),
            timeout_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub agent_type: AgentType,
    pub installation: Option<AgentInstallation>,
    pub probe: Option<ProbeResult>,
    pub health: HealthStatus,
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeEnvelopeInput {
    pub message_id: String,
    pub event_id: String,
    pub correlation_id: String,
    pub project_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub agent_type: AgentType,
    pub adapter_version: String,
    pub protocol_versions: Vec<String>,
    pub capabilities: Vec<String>,
    pub native_transport: Transport,
    pub workspace_id: Option<String>,
    pub project_epoch: i64,
    pub created_at: String,
}
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

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SESSION_STARTED => "SESSION_STARTED",
            Self::TEXT => "TEXT",
            Self::REASONING => "REASONING",
            Self::STEP_START => "STEP_START",
            Self::STEP_FINISH => "STEP_FINISH",
            Self::TOOL_CALL => "TOOL_CALL",
            Self::TOOL_RESULT => "TOOL_RESULT",
            Self::FILE_CHANGE => "FILE_CHANGE",
            Self::COMMAND_REQUEST => "COMMAND_REQUEST",
            Self::COMMAND_RESULT => "COMMAND_RESULT",
            Self::ERROR => "ERROR",
            Self::RESULT => "RESULT",
            Self::SESSION_ENDED => "SESSION_ENDED",
            Self::HEARTBEAT => "HEARTBEAT",
            Self::UNKNOWN => "UNKNOWN",
        }
    }}

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


#[derive(Debug, Clone, Copy, Default)]
pub enum AnyAgentAdapter {
    #[default]
    Hermes,
    Kilo,
    OpenCode,
}

impl AnyAgentAdapter {
    pub const fn for_agent(agent_type: AgentType) -> Self {
        match agent_type {
            AgentType::Hermes => Self::Hermes,
            AgentType::Kilo => Self::Kilo,
            AgentType::OpenCode => Self::OpenCode,
        }
    }
}

impl AgentAdapter for AnyAgentAdapter {
    fn agent_type(&self) -> AgentType {
        match self {
            Self::Hermes => AgentType::Hermes,
            Self::Kilo => AgentType::Kilo,
            Self::OpenCode => AgentType::OpenCode,
        }
    }

    fn prepare_launch(
        &self,
        request: &SessionLaunch,
        version: &AgentVersion,
        prompt: &str,
        native_session_id: Option<&str>,
        proof: LaunchProof,
    ) -> Result<PreparedLaunch, AdapterError> {
        match self {
            Self::Hermes => HermesAdapter.prepare_launch(request,version,prompt,native_session_id,proof),
            Self::Kilo => KiloAdapter.prepare_launch(request,version,prompt,native_session_id,proof),
            Self::OpenCode => OpenCodeAdapter.prepare_launch(request,version,prompt,native_session_id,proof),
        }
    }

    fn normalize_event(&self, raw: &str, received_at: &str, raw_ref: &str)
        -> Result<NativeEvent, AdapterError> {
        match self {
            Self::Hermes => HermesAdapter.normalize_event(raw,received_at,raw_ref),
            Self::Kilo => KiloAdapter.normalize_event(raw,received_at,raw_ref),
            Self::OpenCode => OpenCodeAdapter.normalize_event(raw,received_at,raw_ref),
        }
    }
}

/// Discover and minimally probe one installed agent. This performs only local executable discovery and --version.
/// Interactive credential/auth surfaces and network-capable commands are deliberately not invoked here.

/// Service boundary for agent discovery, session bootstrap and supervised launch.
/// It owns agent-session state; process creation remains in ExecutionService.
pub struct AgentService {
    storage: mayasaba_storage::Storage,
}

impl AgentService {
    pub fn new(storage: mayasaba_storage::Storage) -> Self {
        Self { storage }
    }

    pub fn storage(&self) -> &mayasaba_storage::Storage {
        &self.storage
    }

    pub fn storage_mut(&mut self) -> &mut mayasaba_storage::Storage {
        &mut self.storage
    }

    pub async fn discover_all(
        &self,
        options: &ProbeOptions,
    ) -> Vec<DiscoveryReport> {
        discover_all_agents(options).await
    }

    /// Persist installation and capability facts for one discovered agent.
    /// The report itself is observational and never grants a lease or execution authority.
    pub fn persist_discovery(
        &mut self,
        agent_id: &str,
        report: &DiscoveryReport,
        observed_at: &str,
    ) -> Result<(), mayasaba_storage::StorageError> {
        let installation = report.installation.as_ref().ok_or_else(|| {
            mayasaba_storage::StorageError::Malformed {
                column: "agents".to_string(),
                detail: "cannot persist an absent installation as a discovered agent".to_string(),
            }
        })?;

        self.storage.upsert_agent_installation(
            agent_id,
            installation.agent_type.as_str(),
            &installation.executable,
            Some(&installation.resolved_path),
            Some(&installation.version.raw),
            observed_at,
        )?;

        if let Some(probe) = &report.probe {
            let capabilities_json = serde_json::to_string(&probe.capabilities).map_err(|e| {
                mayasaba_storage::StorageError::Malformed {
                    column: "agent_capabilities.capabilities_json".to_string(),
                    detail: format!("capability serialization failed: {e}"),
                }
            })?;
            self.storage.insert_agent_capability_snapshot(
                &mayasaba_storage::NewAgentCapabilitySnapshot {
                    capability_snapshot_id: format!("{agent_id}_{}", sanitize_id(&probe.version.raw)),
                    agent_id: agent_id.to_owned(),
                    session_id: None,
                    capabilities_json,
                    detected_at: probe.probe_time.clone(),
                },
            )?;
        }
        Ok(())
    }

    /// Create a session and move it through discovery, handshake and capability validation.
    /// Workspace validation remains an external gate owned by WorkspaceService.
    pub fn begin_session(
        &mut self,
        session_id: &str,
        project_id: &str,
        agent_id: &str,
        workspace_id: Option<&str>,
        current_epoch: i64,
        now: &str,
    ) -> Result<mayasaba_storage::AgentSessionRecord, mayasaba_storage::StorageError> {
        let session = self.storage.create_agent_session(&mayasaba_storage::NewAgentSession {
            session_id: session_id.to_owned(),
            project_id: project_id.to_owned(),
            agent_id: agent_id.to_owned(),
            workspace_id: workspace_id.map(ToOwned::to_owned),
            current_epoch,
            started_at: now.to_owned(),
        })?;
        self.storage.transition_agent_session(
            session_id,"DISCOVERED","HANDSHAKING","AGENT_HANDSHAKING",now
        )?;
        self.storage.transition_agent_session(
            session_id,"HANDSHAKING","CAPABILITY_VALIDATING","AGENT_CAPABILITY_VALIDATING",now
        )?;
        self.storage.get_agent_session(session_id)?.ok_or_else(|| {
            mayasaba_storage::StorageError::NotFound(format!("agent session {session_id}"))
        })
    }

    /// Complete capability validation by binding the snapshot to the session and entering workspace validation.
    pub fn complete_capability_validation(
        &mut self,
        session_id: &str,
        capability_snapshot_id: &str,
        capabilities_json: &str,
        detected_at: &str,
    ) -> Result<(), mayasaba_storage::StorageError> {
        let session = self.storage.get_agent_session(session_id)?.ok_or_else(|| {
            mayasaba_storage::StorageError::NotFound(format!("agent session {session_id}"))
        })?;
        self.storage.insert_agent_capability_snapshot(
            &mayasaba_storage::NewAgentCapabilitySnapshot {
                capability_snapshot_id: capability_snapshot_id.to_owned(),
                agent_id: session.agent_id.clone(),
                session_id: Some(session_id.to_owned()),
                capabilities_json: capabilities_json.to_owned(),
                detected_at: detected_at.to_owned(),
            },
        )?;
        self.storage.transition_agent_session(
            session_id,
            "CAPABILITY_VALIDATING",
            "WORKSPACE_VALIDATING",
            "AGENT_WORKSPACE_VALIDATING",
            detected_at,
        )?;
        Ok(())
    }

    pub fn mark_workspace_ready(
        &mut self,
        session_id: &str,
        now: &str,
    ) -> Result<(), mayasaba_storage::StorageError> {
        self.storage.transition_agent_session(
            session_id,
            "WORKSPACE_VALIDATING",
            "READY",
            "AGENT_READY",
            now,
        )
    }
}

fn sanitize_id(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub async fn discover_agent(
    agent_type: AgentType,
    options: &ProbeOptions,
) -> Result<DiscoveryReport, AdapterError> {
    let definition = agent_contract(agent_type)?;
    let executable = definition.get("executable").and_then(Value::as_str).ok_or_else(|| adapter_error(
        AdapterErrorCategory::PROTOCOL,"ADAPTER_EXECUTABLE_MISSING",Retryability::NEVER,
        format!("contract has no executable for {}",agent_type.as_str())))?;

    let where_result = Command::new("where.exe")
        .arg(executable)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).output().await
        .map_err(|e| adapter_error(
            AdapterErrorCategory::DETECTION,"DISCOVERY_COMMAND_FAILED",Retryability::AFTER_USER_ACTION,
            format!("where.exe failed: {e}")))?;

    let resolved_path = String::from_utf8_lossy(&where_result.stdout)
        .lines().map(str::trim).find(|v| !v.is_empty()).map(ToOwned::to_owned);

    let Some(resolved_path) = resolved_path else {
        return Ok(DiscoveryReport {
            agent_type, installation: None, probe: None,
            health: HealthStatus {
                state: HealthState::UNKNOWN,
                checked_at: options.probe_time.clone(),
                detail: Some(format!("{executable} was not found on PATH")),
            },
        });
    };

    let version_tokens = definition.get("version").and_then(Value::as_array).ok_or_else(|| adapter_error(
        AdapterErrorCategory::PROTOCOL,"VERSION_PROBE_MISSING",Retryability::NEVER,
        format!("version probe missing for {}",agent_type.as_str())))?;
    let version_args = version_tokens.iter().map(|v| v.as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| adapter_error(
            AdapterErrorCategory::PROTOCOL,"VERSION_PROBE_INVALID",Retryability::NEVER,
            "version probe contains a non-string token")))
        .collect::<Result<Vec<_>,_>>()?;

    let output = match time::timeout(
        Duration::from_millis(options.timeout_ms.max(100)),
        Command::new(&resolved_path)
            .args(&version_args).current_dir(&options.cwd)
            .stdout(Stdio::piped()).stderr(Stdio::piped()).output()
    ).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Ok(DiscoveryReport {
                agent_type,
                installation: None,
                probe: None,
                health: HealthStatus {
                    state: HealthState::UNHEALTHY,
                    checked_at: options.probe_time.clone(),
                    detail: Some(format!("version probe failed: {e}")),
                },
            })
        }
        Err(_) => {
            return Ok(DiscoveryReport {
                agent_type,
                installation: None,
                probe: None,
                health: HealthStatus {
                    state: HealthState::UNHEALTHY,
                    checked_at: options.probe_time.clone(),
                    detail: Some("version probe timed out".to_owned()),
                },
            })
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = parse_version_from_probe(&format!("{stdout}\n{stderr}"))
        .ok_or_else(|| adapter_error(
            AdapterErrorCategory::DETECTION,"VERSION_PARSE_FAILED",Retryability::AFTER_USER_ACTION,
            format!("could not parse a semantic version from {} output",agent_type.as_str())))?;

    let transport = definition.get("transport").and_then(Value::as_str)
        .and_then(Transport::parse).unwrap_or(Transport::Unsupported);

    let mut capabilities = CapabilitySet::new();
    capabilities.insert("version_probe".into(), true);
    capabilities.insert("local_process".into(), true);
    capabilities.insert("working_directory".into(), true);
    capabilities.insert("structured_transport".into(), transport != Transport::Unsupported);
    capabilities.insert("resume_vector".into(), definition.get("resume").is_some());

    let probe = ProbeResult {
        schema_version: "1.0.0".into(),
        agent_type,
        probe_time: options.probe_time.clone(),
        executable: executable.into(),
        resolved_path: Some(resolved_path.clone()),
        version: version.clone(),
        platform: "windows".into(),
        transport,
        invocation: ProbeInvocation {
            argv: version_args,
            cwd: options.cwd.clone(),
            stdin_mode: "NONE".into(),
            stdout_mode: "CAPTURED".into(),
            stderr_mode: "CAPTURED".into(),
        },
        capabilities,
        exit_semantics: ExitSemantics {
            exit_code: output.status.code(),
            terminated_by_signal: false,
            timeout: false,
        },
        authentication: AuthenticationProbe {
            detected: false,
            method: "NOT_PROBED".into(),
        },
        raw_probe_evidence: vec![bounded_text(&stdout,4096), bounded_text(&stderr,4096)],
        warnings: vec![
            "Credential-bearing or interactive auth commands are not invoked by discovery.".into(),
            "Capability claims are provisional until contract-specific runtime capability probes complete.".into(),
        ],
    };

    Ok(DiscoveryReport {
        agent_type,
        installation: Some(AgentInstallation {
            agent_type, executable: executable.into(), resolved_path, version,
            platform: "windows".into(),
        }),
        probe: Some(probe),
        health: HealthStatus {
            state: if output.status.success() { HealthState::HEALTHY } else { HealthState::UNHEALTHY },
            checked_at: options.probe_time.clone(),
            detail: if output.status.success() { None } else { Some(bounded_text(&stderr,1024)) },
        },
    })
}

pub async fn discover_all_agents(options: &ProbeOptions) -> Vec<DiscoveryReport> {
    let mut reports = Vec::with_capacity(AgentType::ALL.len());
    for agent_type in AgentType::ALL {
        match discover_agent(agent_type, options).await {
            Ok(report) => reports.push(report),
            Err(error) => reports.push(DiscoveryReport {
                agent_type,
                installation: None,
                probe: None,
                health: HealthStatus {
                    state: HealthState::UNHEALTHY,
                    checked_at: options.probe_time.clone(),
                    detail: Some(format!("{}: {}",error.code,error.message)),
                },
            }),
        }
    }
    reports
}

fn parse_version_from_probe(text: &str) -> Option<AgentVersion> {
    text.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+');
        AgentVersion::parse(token)
    })
}

fn bounded_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}


#[derive(Debug)]
pub enum AgentRuntimeError {
    Adapter(AdapterError),
    Execution(mayasaba_execution::ExecutionError),
    Storage(mayasaba_storage::StorageError),
}

impl From<AdapterError> for AgentRuntimeError {
    fn from(value: AdapterError) -> Self { Self::Adapter(value) }
}
impl From<mayasaba_execution::ExecutionError> for AgentRuntimeError {
    fn from(value: mayasaba_execution::ExecutionError) -> Self { Self::Execution(value) }
}
impl From<mayasaba_storage::StorageError> for AgentRuntimeError {
    fn from(value: mayasaba_storage::StorageError) -> Self { Self::Storage(value) }
}

/// A running local agent session. The agent adapter normalizes its stdout events; ExecutionService owns the process.
pub struct LiveAgentSession<A: AgentAdapter> {
    adapter: A,
    process: mayasaba_execution::SpawnedProcess,
    event_sequence: u64,
    session_id: String,
}

impl<A: AgentAdapter> LiveAgentSession<A> {
    pub async fn next_event(
        &mut self,
        storage: &mayasaba_storage::Storage,
        received_at: &str,
    ) -> Result<Option<NativeEvent>, AgentRuntimeError> {
        loop {
            let Some(line) = self.process.next_stdout_line().await? else {
                return Ok(None);
            };
            self.event_sequence = self.event_sequence.saturating_add(1);
            let raw_ref = format!(
                "execution://{}/stdout/{}",
                self.process.execution_id, self.event_sequence
            );
            match self.adapter.normalize_event(&line, received_at, &raw_ref) {
                Ok(event) => {
                    if let Some(native_session_id) = event.native_session_id.as_deref() {
                        storage.bind_agent_process(
                            &self.session_id,
                            i64::from(self.process.pid),
                            Some(native_session_id),
                        )?;
                    }
                    return Ok(Some(event));
                },
                Err(error) => {
                    // Unknown/malformed vendor data must never become a canonical success signal. Preserve the
                    // failure through the adapter error path and require the controller to decide whether recovery
                    // can continue.
                    return Err(AgentRuntimeError::Adapter(error));
                }
            }
        }
    }

    pub async fn stderr_line(&mut self) -> Result<Option<String>, AgentRuntimeError> {
        Ok(self.process.next_stderr_line().await?)
    }

    pub async fn wait(
        self,
        storage: &mayasaba_storage::Storage,
        observed_at: &str,
    ) -> Result<mayasaba_execution::CompletedProcess, AgentRuntimeError> {
        let result = self.process.wait(storage, observed_at).await;
        if result.is_ok() {
            storage.transition_agent_session(
                &self.session_id,
                "ACTIVE",
                "PAUSED",
                "AGENT_PAUSED",
                observed_at,
            )?;
            storage.transition_agent_session(
                &self.session_id,
                "PAUSED",
                "DRAINING",
                "AGENT_DRAINING",
                observed_at,
            )?;
            storage.transition_agent_session(
                &self.session_id,
                "DRAINING",
                "STOPPED",
                "AGENT_STOPPED",
                observed_at,
            )?;
            storage.set_agent_health(&self.session_id, "DEGRADED", observed_at)?;
        }
        result.map_err(Into::into)
    }

    pub async fn wait_timeout(
        self,
        storage: &mayasaba_storage::Storage,
        observed_at: &str,
        timeout: Duration,
    ) -> Result<mayasaba_execution::CompletedProcess, AgentRuntimeError> {
        let result = self.process.wait_timeout(storage, observed_at, timeout).await;
        if let Ok(completed) = &result {
            let health = if completed.timed_out { "UNHEALTHY" } else { "DEGRADED" };
            storage.transition_agent_session(
                &self.session_id,
                "ACTIVE",
                "PAUSED",
                "AGENT_PAUSED",
                observed_at,
            )?;
            storage.transition_agent_session(
                &self.session_id,
                "PAUSED",
                "DRAINING",
                "AGENT_DRAINING",
                observed_at,
            )?;
            storage.transition_agent_session(
                &self.session_id,
                "DRAINING",
                "STOPPED",
                "AGENT_STOPPED",
                observed_at,
            )?;
            storage.set_agent_health(&self.session_id, health, observed_at)?;
        }
        result.map_err(Into::into)
    }
}

/// Launch one agent process after the caller has persisted its command as APPROVED and its session as READY.
/// The execution kernel re-checks the task-attempt fence immediately before spawn; after spawn, this function binds
/// the physical PID and activates the session. Any failure after spawn triggers owned-tree termination.
pub async fn launch_live_session(
    storage: &mut mayasaba_storage::Storage,
    agent: AnyAgentAdapter,
    request: &SessionLaunch,
    version: &AgentVersion,
    prompt: &str,
    native_session_id: Option<&str>,
    proof: LaunchProof,
    execution_id: &str,
    attempt_id: Option<&str>,
    lease_version: Option<i64>,
    started_at: &str,
) -> Result<LiveAgentSession<AnyAgentAdapter>, AgentRuntimeError> {
    let prepared = agent.prepare_launch(request, version, prompt, native_session_id, proof)?;
    let spec = prepared.process_spec();
    let mut process = mayasaba_execution::spawn_process(
        storage,
        execution_id,
        attempt_id,
        lease_version,
        &spec,
        None,
        started_at,
    ).await?;

    if let Err(error) = storage.bind_agent_process(
        &request.session_id,
        i64::from(process.pid),
        None,
    ) {
        let _ = process.terminate().await;
        return Err(AgentRuntimeError::Storage(error));
    }

    storage.set_agent_health(&request.session_id, "HEALTHY", started_at)?;
    let transition_result = storage.transition_agent_session(
        &request.session_id,
        "READY",
        "ACTIVE",
        "AGENT_ACTIVATED",
        started_at,
    );
    if let Err(error) = transition_result {
        let _ = process.terminate().await;
        return Err(AgentRuntimeError::Storage(error));
    }

    Ok(LiveAgentSession {
        adapter: agent,
        process,
        event_sequence: 0,
        session_id: request.session_id.clone(),
    })
}

/// Build a legal MCF-v2 HANDSHAKE envelope from runtime-discovered agent facts.
/// The envelope is passed through the protocol validator before it is released to the bus layer.
pub fn build_handshake_envelope(input: &HandshakeEnvelopeInput) -> Result<String, AdapterError> {
    if input.protocol_versions.is_empty() || !input.protocol_versions.iter().any(|v| v == "MCF-2") {
        return Err(adapter_error(
            AdapterErrorCategory::PROTOCOL,
            "HANDSHAKE_PROTOCOL_VERSION_MISSING",
            Retryability::NEVER,
            "handshake must advertise MCF-2",
        ));
    }
    if input.project_epoch < 0 || input.created_at.trim().is_empty() {
        return Err(adapter_error(
            AdapterErrorCategory::PROTOCOL,
            "HANDSHAKE_METADATA_INVALID",
            Retryability::NEVER,
            "handshake project epoch and created_at must be valid",
        ));
    }
    let payload = serde_json::json!({
        "agent_id": input.agent_id,
        "agent_type": input.agent_type.as_str(),
        "adapter_version": input.adapter_version,
        "protocol_versions": input.protocol_versions,
        "capabilities": input.capabilities,
        "native_transport": input.native_transport.as_str(),
        "workspace_id": input.workspace_id,
    });
    let envelope = serde_json::json!({
        "protocol_version": "MCF-2",
        "schema_version": "2.0.0",
        "message_id": input.message_id,
        "event_id": input.event_id,
        "project_id": input.project_id,
        "session_id": input.session_id,
        "sender": {
            "actor_type": "AGENT",
            "actor_id": input.agent_id,
            "agent_id": input.agent_id,
            "agent_type": input.agent_type.as_str(),
            "session_id": input.session_id,
        },
        "recipients": [{"actor_type":"MAYASABA","actor_id":"mayasaba-controller"}],
        "channel": "agent",
        "message_type": "HANDSHAKE",
        "phase": "UNSCOPED",
        "correlation_id": input.correlation_id,
        "sequence": 0,
        "project_epoch": input.project_epoch,
        "priority": "SYNCHRONIZATION",
        "created_at": input.created_at,
        "requires_ack": true,
        "requires_response": true,
        "blocking": false,
        "payload": payload,
        "security": {
            "classification": "INTERNAL_PROJECT",
            "secret_refs": [],
            "contains_secret_material": false,
        }
    });
    let text = serde_json::to_string(&envelope).map_err(|e| adapter_error(
        AdapterErrorCategory::PROTOCOL, "HANDSHAKE_SERIALIZE_FAILED", Retryability::NEVER,
        format!("cannot serialize handshake envelope: {e}")))?;
    let parsed = mayasaba_protocol::envelope::parse_envelope(&text).map_err(|e| adapter_error(
        AdapterErrorCategory::PROTOCOL, "HANDSHAKE_ENVELOPE_INVALID", Retryability::NEVER,
        format!("protocol validator rejected handshake envelope: {e}")))?;
    Ok(parsed.to_json_text())
}

/// Convert a normalized native event into the canonical MCF message type using the registry.
pub fn native_event_mcf_type(kind: NativeEventKind) -> Option<String> {
    let registry: Value = serde_json::from_str(NATIVE_TO_MCF_REGISTRY).ok()?;
    registry.get("mappings")?.get(kind.as_str())?.as_str().map(ToOwned::to_owned)
}
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
mod tests
 {
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
    fn handshake_builder_refuses_missing_mcf2() {
        let input = HandshakeEnvelopeInput {
            message_id:"11111111-1111-4111-8111-111111111111".into(),
            event_id:"22222222-2222-4222-8222-222222222222".into(),
            correlation_id:"33333333-3333-4333-8333-333333333333".into(),
            project_id:"44444444-4444-4444-8444-444444444444".into(),
            session_id:"55555555-5555-4555-8555-555555555555".into(),
            agent_id:"66666666-6666-4666-8666-666666666666".into(),
            agent_type:AgentType::Hermes,
            adapter_version:"1.0.0".into(),
            protocol_versions:vec!["MCF-1".into()],
            capabilities:vec![],
            native_transport:Transport::HermesStreamJson,
            workspace_id:None,
            project_epoch:0,
            created_at:"2026-10-05T17:00:00Z".into(),
        };
        let error = build_handshake_envelope(&input).expect_err("MCF-2 is mandatory");
        assert_eq!(error.code,"HANDSHAKE_PROTOCOL_VERSION_MISSING");
    }

    #[test]
    fn handshake_builder_produces_protocol_valid_envelope() {
        let input = HandshakeEnvelopeInput {
            message_id:"11111111-1111-4111-8111-111111111111".into(),
            event_id:"22222222-2222-4222-8222-222222222222".into(),
            correlation_id:"33333333-3333-4333-8333-333333333333".into(),
            project_id:"44444444-4444-4444-8444-444444444444".into(),
            session_id:"55555555-5555-4555-8555-555555555555".into(),
            agent_id:"66666666-6666-4666-8666-666666666666".into(),
            agent_type:AgentType::Kilo,
            adapter_version:"1.0.0".into(),
            protocol_versions:vec!["MCF-2".into()],
            capabilities:vec!["version_probe".into(),"structured_transport".into()],
            native_transport:Transport::KiloJson,
            workspace_id:Some("77777777-7777-4777-8777-777777777777".into()),
            project_epoch:2,
            created_at:"2026-10-05T17:00:00Z".into(),
        };
        let text = build_handshake_envelope(&input).expect("valid");
        let parsed = mayasaba_protocol::envelope::parse_envelope(&text).expect("validator");
        assert_eq!(parsed.message_type(),"HANDSHAKE");
        assert_eq!(parsed.project_epoch(),2);
        assert_eq!(parsed.channel(),"agent");
    }

    #[test]
    fn native_mapping_comes_from_registry_and_preserves_unmapped_telemetry() {
        assert_eq!(native_event_mcf_type(NativeEventKind::SESSION_STARTED).as_deref(),Some("HANDSHAKE_ACK"));
        assert_eq!(native_event_mcf_type(NativeEventKind::HEARTBEAT).as_deref(),Some("HEARTBEAT"));
        assert_eq!(native_event_mcf_type(NativeEventKind::REASONING),None);
        assert_eq!(native_event_mcf_type(NativeEventKind::UNKNOWN),None);
    }

    #[test]
    fn probe_version_parser_accepts_vendor_prefixes() {
        assert_eq!((super::parse_version_from_probe("Hermes Agent 0.21.5").unwrap().major,
                    super::parse_version_from_probe("Hermes Agent 0.21.5").unwrap().minor), (0,21));
        assert_eq!(super::parse_version_from_probe("v1.18.34").unwrap().line(), "1.x");
        assert_eq!(super::parse_version_from_probe("release 2.0.22-beta").unwrap().prerelease.as_deref(), Some("beta"));
        assert!(super::parse_version_from_probe("no version here").is_none());
    }

    #[test]
    fn any_adapter_routes_to_the_closed_agent_set() {
        assert_eq!(AnyAgentAdapter::for_agent(AgentType::Hermes).agent_type(), AgentType::Hermes);
        assert_eq!(AnyAgentAdapter::for_agent(AgentType::Kilo).agent_type(), AgentType::Kilo);
        assert_eq!(AnyAgentAdapter::for_agent(AgentType::OpenCode).agent_type(), AgentType::OpenCode);
    }

    #[test]
    fn discovery_options_have_a_bounded_default_probe_window() {
        let options = ProbeOptions::default();
        assert_eq!(options.timeout_ms, 10_000);
        assert!(options.timeout_ms >= 100);
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
