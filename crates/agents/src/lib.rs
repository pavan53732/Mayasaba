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
use tokio::{
    process::Command,
    time::{self, Duration},
};

const NATIVE_TRANSPORT_CONTRACT: &str =
    include_str!("../../../schemas/agent-adapter-v1/native-transport-contract.json");
const NATIVE_TO_MCF_REGISTRY: &str =
    include_str!("../../../schemas/agent-adapter-v1/native-to-mcf.registry.json");

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
        Some(Self {
            raw,
            major,
            minor,
            patch,
            prerelease,
        })
    }

    pub const fn line(&self) -> &'static str {
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

/// The exact invocation a probe performed, so its evidence can be re-read without re-running it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeInvocation {
    pub argv: Vec<String>,
    pub cwd: String,
    pub stdin_mode: String,
    pub stdout_mode: String,
    pub stderr_mode: String,
}

/// How the probe process ended. A signal or timeout is not an exit code, so they are recorded separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitSemantics {
    pub exit_code: Option<i32>,
    pub terminated_by_signal: bool,
    pub timeout: bool,
}

/// Authentication surface observed during probing.
///
/// Discovery never invokes credential-bearing or interactive auth commands, so `detected` stays false and
/// `method` stays `NOT_PROBED` unless a future non-interactive probe establishes otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationProbe {
    pub detected: bool,
    pub method: String,
}

/// Evidence captured from one local agent version probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub schema_version: String,
    pub agent_type: AgentType,
    pub probe_time: String,
    pub executable: String,
    pub resolved_path: Option<String>,
    pub version: AgentVersion,
    pub platform: String,
    pub transport: Transport,
    pub invocation: ProbeInvocation,
    pub capabilities: CapabilitySet,
    pub exit_semantics: ExitSemantics,
    pub authentication: AuthenticationProbe,
    pub raw_probe_evidence: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeOptions {
    pub cwd: String,
    pub probe_time: String,
    pub timeout_ms: u64,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir()
                .ok()
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

impl NativeEventKind {
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
    }
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
            Self::Hermes => {
                HermesAdapter.prepare_launch(request, version, prompt, native_session_id, proof)
            }
            Self::Kilo => {
                KiloAdapter.prepare_launch(request, version, prompt, native_session_id, proof)
            }
            Self::OpenCode => {
                OpenCodeAdapter.prepare_launch(request, version, prompt, native_session_id, proof)
            }
        }
    }

    fn normalize_event(
        &self,
        raw: &str,
        received_at: &str,
        raw_ref: &str,
    ) -> Result<NativeEvent, AdapterError> {
        match self {
            Self::Hermes => HermesAdapter.normalize_event(raw, received_at, raw_ref),
            Self::Kilo => KiloAdapter.normalize_event(raw, received_at, raw_ref),
            Self::OpenCode => OpenCodeAdapter.normalize_event(raw, received_at, raw_ref),
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

    pub async fn discover_all(&self, options: &ProbeOptions) -> Vec<DiscoveryReport> {
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
                    capability_snapshot_id: format!(
                        "{agent_id}_{}",
                        sanitize_id(&probe.version.raw)
                    ),
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
        let session = self
            .storage
            .create_agent_session(&mayasaba_storage::NewAgentSession {
                session_id: session_id.to_owned(),
                project_id: project_id.to_owned(),
                agent_id: agent_id.to_owned(),
                workspace_id: workspace_id.map(ToOwned::to_owned),
                current_epoch,
                started_at: now.to_owned(),
            })?;
        self.storage.transition_agent_session(
            session_id,
            "DISCOVERED",
            "HANDSHAKING",
            "AGENT_HANDSHAKING",
            now,
        )?;
        self.storage.transition_agent_session(
            session_id,
            "HANDSHAKING",
            "CAPABILITY_VALIDATING",
            "AGENT_CAPABILITY_VALIDATING",
            now,
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
        let capabilities: Value = serde_json::from_str(capabilities_json).map_err(|e| {
            mayasaba_storage::StorageError::Malformed {
                column: "agent_capabilities.capabilities_json".to_string(),
                detail: format!("capability snapshot is not valid JSON: {e}"),
            }
        })?;
        let contract_verified = capabilities
            .get("contract_surface_verified")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let launch_verified = capabilities
            .get("launch_surface_verified")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let resume_verified = capabilities
            .get("resume_surface_verified")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !(contract_verified && launch_verified && resume_verified) {
            return Err(mayasaba_storage::StorageError::Malformed {
                column: "agent_capabilities.capabilities_json".to_string(),
                detail: "capability validation requires contract_surface_verified, launch_surface_verified and resume_surface_verified=true".to_string(),
            });
        }
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
    let executable = definition
        .get("executable")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            adapter_error(
                AdapterErrorCategory::PROTOCOL,
                "ADAPTER_EXECUTABLE_MISSING",
                Retryability::NEVER,
                format!("contract has no executable for {}", agent_type.as_str()),
            )
        })?;

    let where_result = Command::new("where.exe")
        .arg(executable)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| {
            adapter_error(
                AdapterErrorCategory::DETECTION,
                "DISCOVERY_COMMAND_FAILED",
                Retryability::AFTER_USER_ACTION,
                format!("where.exe failed: {e}"),
            )
        })?;

    let resolved_path = String::from_utf8_lossy(&where_result.stdout)
        .lines()
        .map(str::trim)
        .find(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    let Some(resolved_path) = resolved_path else {
        return Ok(DiscoveryReport {
            agent_type,
            installation: None,
            probe: None,
            health: HealthStatus {
                state: HealthState::UNKNOWN,
                checked_at: options.probe_time.clone(),
                detail: Some(format!("{executable} was not found on PATH")),
            },
        });
    };

    let version_tokens = definition
        .get("version")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            adapter_error(
                AdapterErrorCategory::PROTOCOL,
                "VERSION_PROBE_MISSING",
                Retryability::NEVER,
                format!("version probe missing for {}", agent_type.as_str()),
            )
        })?;
    let version_args = version_tokens
        .iter()
        .map(|v| {
            v.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                adapter_error(
                    AdapterErrorCategory::PROTOCOL,
                    "VERSION_PROBE_INVALID",
                    Retryability::NEVER,
                    "version probe contains a non-string token",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let output = match time::timeout(
        Duration::from_millis(options.timeout_ms.max(100)),
        Command::new(&resolved_path)
            .args(&version_args)
            .current_dir(&options.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    {
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
    let version = parse_version_from_probe(&format!("{stdout}\n{stderr}")).ok_or_else(|| {
        adapter_error(
            AdapterErrorCategory::DETECTION,
            "VERSION_PARSE_FAILED",
            Retryability::AFTER_USER_ACTION,
            format!(
                "could not parse a semantic version from {} output",
                agent_type.as_str()
            ),
        )
    })?;

    let transport = definition
        .get("transport")
        .and_then(Value::as_str)
        .and_then(Transport::parse)
        .unwrap_or(Transport::Unsupported);

    let help_probe = time::timeout(
        Duration::from_millis(options.timeout_ms.max(100)),
        Command::new(&resolved_path)
            .arg("--help")
            .current_dir(&options.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await;

    let (help_ok, help_text) = match help_probe {
        Ok(Ok(output)) => {
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            (output.status.success(), text)
        }
        Ok(Err(_)) | Err(_) => (false, String::new()),
    };

    let launch_surface_verified =
        help_ok && contract_surface_present(&definition, "launch", &help_text);
    let resume_surface_verified =
        help_ok && contract_surface_present(&definition, "resume", &help_text);

    // Capability admission is derived from the canonical adapter contract plus the local binary's help surface.
    // The probe never executes a real task: it validates that the contract can construct an admitted launch,
    // that its resume vector is structurally valid, and that the generated launch surface contains none of
    // the contract's forbidden remote commands/flags.
    let probe_request = SessionLaunch {
        project_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        session_id: "22222222-2222-4222-8222-222222222222".to_owned(),
        workspace_id: "33333333-3333-4333-8333-333333333333".to_owned(),
        cwd: options.cwd.clone(),
        transport,
        prompt_ref: None,
    };
    let contract_probe = AnyAgentAdapter::for_agent(agent_type).prepare_launch(
        &probe_request,
        &version,
        "mayasaba-capability-probe",
        None,
        LaunchProof {
            workspace_validated: true,
            policy_validated: true,
            capability_validated: true,
            transport_validated: true,
            native_configuration_validated: true,
        },
    );
    let contract_surface_verified = match contract_probe {
        Ok(prepared) => {
            prepared
                .argv
                .iter()
                .all(|token| token != "mayasaba-capability-probe")
                && launch_vector_has_no_forbidden_remote_surface(
                    &definition,
                    &prepared.argv,
                    "mayasaba-capability-probe",
                )
        }
        Err(_) => false,
    };

    let mut capabilities = CapabilitySet::new();
    capabilities.insert("version_probe".into(), true);
    capabilities.insert("local_process".into(), true);
    capabilities.insert("working_directory".into(), true);
    capabilities.insert(
        "structured_transport".into(),
        transport != Transport::Unsupported,
    );
    capabilities.insert("resume_vector".into(), definition.get("resume").is_some());
    capabilities.insert("launch_surface_verified".into(), launch_surface_verified);
    capabilities.insert("resume_surface_verified".into(), resume_surface_verified);
    capabilities.insert(
        "contract_surface_verified".into(),
        contract_surface_verified,
    );

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
            "Capability admission is accepted only from the contract-derived launch/resume/safety probe; credential-bearing runtime surfaces are not invoked during discovery.".into(),
            format!(
                "launch_surface_verified={launch_surface_verified}; resume_surface_verified={resume_surface_verified}"
            ),
        ],
    };

    Ok(DiscoveryReport {
        agent_type,
        installation: Some(AgentInstallation {
            agent_type,
            executable: executable.into(),
            resolved_path,
            version,
            platform: "windows".into(),
        }),
        probe: Some(probe),
        health: HealthStatus {
            state: if output.status.success() {
                HealthState::HEALTHY
            } else {
                HealthState::UNHEALTHY
            },
            checked_at: options.probe_time.clone(),
            detail: if output.status.success() {
                None
            } else {
                Some(bounded_text(&stderr, 1024))
            },
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
                    detail: Some(format!("{}: {}", error.code, error.message)),
                },
            }),
        }
    }
    reports
}

fn launch_vector_has_no_forbidden_remote_surface(
    definition: &Value,
    argv: &[String],
    prompt: &str,
) -> bool {
    let static_tokens: Vec<&str> = argv
        .iter()
        .map(String::as_str)
        .filter(|token| *token != prompt)
        .collect();

    let forbidden_flags = definition
        .get("remote_forbidden_flags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);

    if forbidden_flags
        .clone()
        .any(|flag| static_tokens.iter().any(|token| token == &flag))
    {
        return false;
    }

    let mut forbidden_commands = Vec::new();
    for key in ["remote_forbidden_commands", "remote_forbidden_subcommands"] {
        if let Some(values) = definition.get(key).and_then(Value::as_array) {
            for value in values.iter().filter_map(Value::as_str) {
                forbidden_commands.push(value.to_ascii_lowercase());
            }
        }
    }

    let joined = static_tokens.join(" ").to_ascii_lowercase();
    !forbidden_commands
        .iter()
        .any(|command| joined.contains(command))
}

fn contract_surface_present(definition: &Value, key: &str, help_text: &str) -> bool {
    let Some(values) = definition.get(key).and_then(Value::as_array) else {
        return false;
    };
    let lower = help_text.to_ascii_lowercase();
    let mut checked = false;
    for value in values {
        let Some(token) = value.as_str() else {
            continue;
        };
        if token.starts_with('<') {
            continue;
        }
        if token.starts_with('-') || token == "chat" || token == "run" || token == "acp" {
            checked = true;
            if !lower.contains(&token.to_ascii_lowercase()) {
                return false;
            }
        }
    }
    checked
}

fn parse_version_from_probe(text: &str) -> Option<AgentVersion> {
    text.split_whitespace().find_map(|token| {
        let token = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+');
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
    fn from(value: AdapterError) -> Self {
        Self::Adapter(value)
    }
}
impl From<mayasaba_execution::ExecutionError> for AgentRuntimeError {
    fn from(value: mayasaba_execution::ExecutionError) -> Self {
        Self::Execution(value)
    }
}
impl From<mayasaba_storage::StorageError> for AgentRuntimeError {
    fn from(value: mayasaba_storage::StorageError) -> Self {
        Self::Storage(value)
    }
}

/// A running local agent session. The agent adapter normalizes its stdout events; ExecutionService owns the process.
pub struct LiveAgentSession<A: AgentAdapter> {
    adapter: A,
    process: mayasaba_execution::SpawnedProcess,
    event_sequence: u64,
    session_id: String,
    native_session_id: Option<String>,
}

impl<A: AgentAdapter> LiveAgentSession<A> {
    pub async fn next_event(
        &mut self,
        storage: &mayasaba_storage::Storage,
        received_at: &str,
    ) -> Result<Option<NativeEvent>, AgentRuntimeError> {
        // One line yields exactly one event or one error: every arm below returns, so this is a straight-line
        // read rather than a loop. It was previously wrapped in `loop { .. }` whose body never iterated, which
        // `clippy::never_loop` rejects as a correctness defect. The wrapper is removed rather than restructured,
        // because no arm ever wanted to continue.
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
                    if native_session_id.trim().is_empty() {
                        return Err(AgentRuntimeError::Adapter(adapter_error(
                            AdapterErrorCategory::PROTOCOL,
                            "ADAPTER_PROTOCOL_ERROR",
                            Retryability::NEVER,
                            "native event carried an empty native session id",
                        )));
                    }
                    if let Some(expected) = self.native_session_id.as_deref() {
                        if expected != native_session_id {
                            return Err(AgentRuntimeError::Adapter(adapter_error(
                                AdapterErrorCategory::PROTOCOL,
                                "ADAPTER_PROTOCOL_ERROR",
                                Retryability::NEVER,
                                format!("native session id changed mid-session: expected {expected}, observed {native_session_id}"),
                            )));
                        }
                    } else {
                        self.native_session_id = Some(native_session_id.to_owned());
                    }
                    storage.bind_agent_process(
                        &self.session_id,
                        i64::from(self.process.pid),
                        self.native_session_id.as_deref(),
                    )?;
                }
                Ok(Some(event))
            }
            Err(error) => {
                // Unknown/malformed vendor data must never become a canonical success signal. Preserve the
                // failure through the adapter error path and require the controller to decide whether recovery
                // can continue.
                Err(AgentRuntimeError::Adapter(error))
            }
        }
    }

    pub async fn stderr_line(&mut self) -> Result<Option<String>, AgentRuntimeError> {
        Ok(self.process.next_stderr_line().await?)
    }

    /// Reconcile runtime health against the supervised OS process, not just the durable health flag.
    pub async fn health_check(
        &mut self,
        storage: &mut mayasaba_storage::Storage,
        checked_at: &str,
    ) -> Result<HealthStatus, AgentRuntimeError> {
        match self.process.try_wait().await {
            Ok(None) => {
                storage.set_agent_health(&self.session_id, "HEALTHY", checked_at)?;
                Ok(HealthStatus {
                    state: HealthState::HEALTHY,
                    checked_at: checked_at.to_owned(),
                    detail: None,
                })
            }
            Ok(Some(status)) => {
                storage.set_agent_health(&self.session_id, "UNHEALTHY", checked_at)?;
                Ok(HealthStatus {
                    state: HealthState::UNHEALTHY,
                    checked_at: checked_at.to_owned(),
                    detail: Some(format!(
                        "supervised process exited with {:?}",
                        status.code()
                    )),
                })
            }
            Err(error) => {
                storage.set_agent_health(&self.session_id, "UNKNOWN", checked_at)?;
                Ok(HealthStatus {
                    state: HealthState::UNKNOWN,
                    checked_at: checked_at.to_owned(),
                    detail: Some(format!("process liveness could not be observed: {error}")),
                })
            }
        }
    }

    pub async fn wait(
        self,
        storage: &mut mayasaba_storage::Storage,
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
        storage: &mut mayasaba_storage::Storage,
        observed_at: &str,
        timeout: Duration,
    ) -> Result<mayasaba_execution::CompletedProcess, AgentRuntimeError> {
        let result = self
            .process
            .wait_timeout(storage, observed_at, timeout)
            .await;
        if let Ok(completed) = &result {
            let health = if completed.timed_out {
                "UNHEALTHY"
            } else {
                "DEGRADED"
            };
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
    )
    .await?;

    if let Err(error) = storage.set_agent_health(&request.session_id, "HEALTHY", started_at) {
        let _ = process.terminate().await;
        return Err(AgentRuntimeError::Storage(error));
    }

    if let Err(error) = storage.activate_agent_process(
        &request.session_id,
        i64::from(process.pid),
        native_session_id,
        started_at,
    ) {
        let _ = process.terminate().await;
        return Err(AgentRuntimeError::Storage(error));
    }

    Ok(LiveAgentSession {
        adapter: agent,
        process,
        event_sequence: 0,
        session_id: request.session_id.clone(),
        native_session_id: native_session_id.map(ToOwned::to_owned),
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
    let text = serde_json::to_string(&envelope).map_err(|e| {
        adapter_error(
            AdapterErrorCategory::PROTOCOL,
            "HANDSHAKE_SERIALIZE_FAILED",
            Retryability::NEVER,
            format!("cannot serialize handshake envelope: {e}"),
        )
    })?;
    let parsed = mayasaba_protocol::envelope::parse_envelope(&text).map_err(|e| {
        adapter_error(
            AdapterErrorCategory::PROTOCOL,
            "HANDSHAKE_ENVELOPE_INVALID",
            Retryability::NEVER,
            format!("protocol validator rejected handshake envelope: {e}"),
        )
    })?;
    Ok(parsed.to_json_text())
}

/// Convert a normalized native event into the canonical MCF message type using the registry.
pub fn native_event_mcf_type(kind: NativeEventKind) -> Option<String> {
    let registry: Value = serde_json::from_str(NATIVE_TO_MCF_REGISTRY).ok()?;
    registry
        .get("mappings")?
        .get(kind.as_str())?
        .as_str()
        .map(ToOwned::to_owned)
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
    fn agent_type(&self) -> AgentType {
        AgentType::Hermes
    }
    fn prepare_launch(
        &self,
        request: &SessionLaunch,
        version: &AgentVersion,
        prompt: &str,
        native_session_id: Option<&str>,
        proof: LaunchProof,
    ) -> Result<PreparedLaunch, AdapterError> {
        prepare_from_contract(
            AgentType::Hermes,
            request,
            version,
            prompt,
            native_session_id,
            proof,
        )
    }
    fn normalize_event(
        &self,
        raw: &str,
        received_at: &str,
        raw_ref: &str,
    ) -> Result<NativeEvent, AdapterError> {
        normalize_native_event(AgentType::Hermes, raw, received_at, raw_ref)
    }
}

impl AgentAdapter for KiloAdapter {
    fn agent_type(&self) -> AgentType {
        AgentType::Kilo
    }
    fn prepare_launch(
        &self,
        request: &SessionLaunch,
        version: &AgentVersion,
        prompt: &str,
        native_session_id: Option<&str>,
        proof: LaunchProof,
    ) -> Result<PreparedLaunch, AdapterError> {
        prepare_from_contract(
            AgentType::Kilo,
            request,
            version,
            prompt,
            native_session_id,
            proof,
        )
    }
    fn normalize_event(
        &self,
        raw: &str,
        received_at: &str,
        raw_ref: &str,
    ) -> Result<NativeEvent, AdapterError> {
        normalize_native_event(AgentType::Kilo, raw, received_at, raw_ref)
    }
}

impl AgentAdapter for OpenCodeAdapter {
    fn agent_type(&self) -> AgentType {
        AgentType::OpenCode
    }
    fn prepare_launch(
        &self,
        request: &SessionLaunch,
        version: &AgentVersion,
        prompt: &str,
        native_session_id: Option<&str>,
        proof: LaunchProof,
    ) -> Result<PreparedLaunch, AdapterError> {
        prepare_from_contract(
            AgentType::OpenCode,
            request,
            version,
            prompt,
            native_session_id,
            proof,
        )
    }
    fn normalize_event(
        &self,
        raw: &str,
        received_at: &str,
        raw_ref: &str,
    ) -> Result<NativeEvent, AdapterError> {
        normalize_native_event(AgentType::OpenCode, raw, received_at, raw_ref)
    }
}

fn adapter_error(
    category: AdapterErrorCategory,
    code: &'static str,
    retryability: Retryability,
    message: impl Into<String>,
) -> AdapterError {
    AdapterError {
        category,
        code: code.to_owned(),
        retryability,
        message: message.into(),
        causation_id: None,
    }
}

fn contract() -> Result<Value, AdapterError> {
    serde_json::from_str(NATIVE_TRANSPORT_CONTRACT).map_err(|e| {
        adapter_error(
            AdapterErrorCategory::PROTOCOL,
            "ADAPTER_CONTRACT_INVALID",
            Retryability::NEVER,
            format!("native transport contract is invalid JSON: {e}"),
        )
    })
}

fn agent_contract(agent: AgentType) -> Result<Value, AdapterError> {
    contract()?
        .get("agents")
        .and_then(|v| v.get(agent.as_str()))
        .cloned()
        .ok_or_else(|| {
            adapter_error(
                AdapterErrorCategory::PROTOCOL,
                "ADAPTER_CONTRACT_MISSING_AGENT",
                Retryability::NEVER,
                format!("native transport contract has no {}", agent.as_str()),
            )
        })
}

fn expand_vector(
    definition: &Value,
    resume: bool,
    prompt: &str,
    workspace: &str,
    native_session_id: Option<&str>,
) -> Result<Vec<String>, AdapterError> {
    let key = if resume { "resume" } else { "launch" };
    let vector = definition
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            adapter_error(
                AdapterErrorCategory::PROTOCOL,
                "ADAPTER_LAUNCH_VECTOR_MISSING",
                Retryability::NEVER,
                format!("native transport contract has no {key} vector"),
            )
        })?;
    vector
        .iter()
        .map(|token| {
            let token = token.as_str().ok_or_else(|| {
                adapter_error(
                    AdapterErrorCategory::PROTOCOL,
                    "ADAPTER_LAUNCH_TOKEN_INVALID",
                    Retryability::NEVER,
                    "launch vector contains a non-string token",
                )
            })?;
            match token {
                "<prompt>" => Ok(prompt.to_owned()),
                "<workspace>" => Ok(workspace.to_owned()),
                "<native_session_id>" => {
                    native_session_id.map(ToOwned::to_owned).ok_or_else(|| {
                        adapter_error(
                            AdapterErrorCategory::LAUNCH,
                            "NATIVE_SESSION_REQUIRED",
                            Retryability::AFTER_SYNC,
                            "resume requested but native session identity is absent",
                        )
                    })
                }
                value => Ok(value.to_owned()),
            }
        })
        .collect()
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
            AdapterErrorCategory::WORKSPACE,
            "WORKSPACE_PATH_NOT_ABSOLUTE",
            Retryability::AFTER_USER_ACTION,
            "agent launch cwd must be an absolute Windows path",
        ));
    }
    if request.prompt_ref.is_none() && prompt.trim().is_empty() {
        return Err(adapter_error(
            AdapterErrorCategory::LAUNCH,
            "PROMPT_EMPTY",
            Retryability::NEVER,
            "agent launch requires a non-empty prompt or prompt_ref",
        ));
    }

    let definition = agent_contract(agent)?;
    if let Some(lines) = definition
        .get("version_gate")
        .and_then(|v| v.get("admitted_lines"))
        .and_then(Value::as_array)
    {
        if !lines.iter().any(|v| v.as_str() == Some(version.line())) {
            return Err(adapter_error(
                AdapterErrorCategory::CAPABILITY,
                "AGENT_VERSION_UNADMITTED",
                Retryability::AFTER_USER_ACTION,
                format!(
                    "{} version {} is not an admitted native line",
                    agent.as_str(),
                    version.raw
                ),
            ));
        }
    }

    let resume = native_session_id.is_some();
    let mut argv = expand_vector(&definition, resume, prompt, &request.cwd, native_session_id)?;

    if let Some(flags) = definition.get("isolation_flags").and_then(Value::as_array) {
        argv.extend(
            flags
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned),
        );
    }

    let mut environment = Environment::new();
    if let Some(required) = definition
        .get("required_environment")
        .and_then(Value::as_object)
    {
        for (key, value) in required {
            if let Some(value) = value.as_str() {
                environment.insert(key.clone(), value.to_owned());
            }
        }
    }

    let mut required_config = definition
        .get("required_config")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    // required_config_injection is contract metadata. Only its actual config-bearing fields belong in the
    // vendor config document: the permission map is inserted as "permission", while audit rationale and probe
    // instructions remain controller metadata and must never be handed to the CLI as unknown config keys.
    if let Some(injection) = definition.get("required_config_injection") {
        let channel = injection
            .get("channel")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                adapter_error(
                    AdapterErrorCategory::PROTOCOL,
                    "CONFIG_INJECTION_CHANNEL_MISSING",
                    Retryability::NEVER,
                    "required_config_injection has no channel",
                )
            })?;
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
        let encoded = serde_json::to_string(&required_config).map_err(|e| {
            adapter_error(
                AdapterErrorCategory::PROTOCOL,
                "CONFIG_INJECTION_SERIALIZE_FAILED",
                Retryability::NEVER,
                format!("cannot serialize effective config document: {e}"),
            )
        })?;
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

    let transport_text = definition
        .get("transport")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            adapter_error(
                AdapterErrorCategory::PROTOCOL,
                "ADAPTER_TRANSPORT_MISSING",
                Retryability::NEVER,
                "native transport contract has no transport",
            )
        })?;
    let transport = Transport::parse(transport_text).ok_or_else(|| {
        adapter_error(
            AdapterErrorCategory::PROTOCOL,
            "ADAPTER_TRANSPORT_UNKNOWN",
            Retryability::NEVER,
            format!("unknown transport {transport_text}"),
        )
    })?;
    if transport != request.transport {
        return Err(adapter_error(
            AdapterErrorCategory::TRANSPORT,
            "TRANSPORT_MISMATCH",
            Retryability::AFTER_SYNC,
            format!(
                "request transport {:?} does not match contract transport {:?}",
                request.transport, transport
            ),
        ));
    }

    Ok(PreparedLaunch {
        agent_type: agent,
        executable: definition
            .get("executable")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
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
            AdapterErrorCategory::PARSE,
            "EVENT_PROVENANCE_MISSING",
            Retryability::NEVER,
            "normalized native events require received_at and raw_ref",
        ));
    }
    let payload: Value = serde_json::from_str(raw).map_err(|e| {
        adapter_error(
            AdapterErrorCategory::PARSE,
            "NATIVE_EVENT_NOT_JSON",
            Retryability::NEVER,
            format!("native event is not JSON: {e}"),
        )
    })?;
    let ty = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

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

    let native_session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
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
    (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || path.starts_with("\\\\")
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
        assert_eq!(
            AgentType::ALL.map(AgentType::as_str),
            ["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"]
        );
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
        let error = OpenCodeAdapter
            .prepare_launch(
                &request(Transport::OpenCodeJson),
                &AgentVersion::parse("2.0.22").unwrap(),
                "work",
                None,
                proof(),
            )
            .expect_err("the current contract admits only OpenCode 1.x");
        assert_eq!(error.code, "AGENT_VERSION_UNADMITTED");
    }

    #[test]
    fn hermes_launch_uses_canonical_contract() {
        let prepared = HermesAdapter
            .prepare_launch(
                &request(Transport::HermesStreamJson),
                &AgentVersion::parse("0.21.5").unwrap(),
                "work",
                None,
                proof(),
            )
            .expect("prepared");
        assert_eq!(prepared.executable, "hermes");
        assert!(prepared.argv.contains(&"--ignore-rules".to_owned()));
    }

    #[test]
    fn kilo_launch_carries_required_local_restrictions() {
        let prepared = KiloAdapter
            .prepare_launch(
                &request(Transport::KiloJson),
                &AgentVersion::parse("7.8.1").unwrap(),
                "work",
                None,
                proof(),
            )
            .expect("prepared");
        assert_eq!(prepared.executable, "kilo");
        assert_eq!(
            prepared.environment.get("KILO_DISABLE_SHARE"),
            Some(&"1".to_owned())
        );
        assert!(prepared.environment.contains_key("KILO_CONFIG_CONTENT"));
        assert!(prepared
            .scrub_inherited_environment
            .contains(&"PWD".to_owned()));
        assert_eq!(
            prepared
                .required_config
                .get("share")
                .and_then(Value::as_str),
            Some("disabled")
        );
        assert!(prepared.required_config.get("permission").is_some());
        assert!(prepared.required_config.get("rationale").is_none());
    }

    #[test]
    fn incomplete_proof_is_fail_closed() {
        let bad = LaunchProof {
            transport_validated: false,
            ..proof()
        };
        let error = HermesAdapter
            .prepare_launch(
                &request(Transport::HermesStreamJson),
                &AgentVersion::parse("0.21.5").unwrap(),
                "work",
                None,
                bad,
            )
            .expect_err("missing gate proof must refuse");
        assert_eq!(error.code, "LAUNCH_GATE_INCOMPLETE");
    }

    #[test]
    fn handshake_builder_refuses_missing_mcf2() {
        let input = HandshakeEnvelopeInput {
            message_id: "11111111-1111-4111-8111-111111111111".into(),
            event_id: "22222222-2222-4222-8222-222222222222".into(),
            correlation_id: "33333333-3333-4333-8333-333333333333".into(),
            project_id: "44444444-4444-4444-8444-444444444444".into(),
            session_id: "55555555-5555-4555-8555-555555555555".into(),
            agent_id: "66666666-6666-4666-8666-666666666666".into(),
            agent_type: AgentType::Hermes,
            adapter_version: "1.0.0".into(),
            protocol_versions: vec!["MCF-1".into()],
            capabilities: vec![],
            native_transport: Transport::HermesStreamJson,
            workspace_id: None,
            project_epoch: 0,
            created_at: "2026-10-05T17:00:00Z".into(),
        };
        let error = build_handshake_envelope(&input).expect_err("MCF-2 is mandatory");
        assert_eq!(error.code, "HANDSHAKE_PROTOCOL_VERSION_MISSING");
    }

    #[test]
    fn handshake_builder_produces_protocol_valid_envelope() {
        let input = HandshakeEnvelopeInput {
            message_id: "11111111-1111-4111-8111-111111111111".into(),
            event_id: "22222222-2222-4222-8222-222222222222".into(),
            correlation_id: "33333333-3333-4333-8333-333333333333".into(),
            project_id: "44444444-4444-4444-8444-444444444444".into(),
            session_id: "55555555-5555-4555-8555-555555555555".into(),
            agent_id: "66666666-6666-4666-8666-666666666666".into(),
            agent_type: AgentType::Kilo,
            adapter_version: "1.0.0".into(),
            protocol_versions: vec!["MCF-2".into()],
            capabilities: vec!["version_probe".into(), "structured_transport".into()],
            native_transport: Transport::KiloJson,
            workspace_id: Some("77777777-7777-4777-8777-777777777777".into()),
            project_epoch: 2,
            created_at: "2026-10-05T17:00:00Z".into(),
        };
        let text = build_handshake_envelope(&input).expect("valid");
        let parsed = mayasaba_protocol::envelope::parse_envelope(&text).expect("validator");
        assert_eq!(parsed.message_type(), "HANDSHAKE");
        assert_eq!(parsed.project_epoch(), 2);
        assert_eq!(parsed.channel(), "agent");
    }

    #[test]
    fn native_mapping_comes_from_registry_and_preserves_unmapped_telemetry() {
        assert_eq!(
            native_event_mcf_type(NativeEventKind::SESSION_STARTED).as_deref(),
            Some("HANDSHAKE_ACK")
        );
        assert_eq!(
            native_event_mcf_type(NativeEventKind::HEARTBEAT).as_deref(),
            Some("HEARTBEAT")
        );
        assert_eq!(native_event_mcf_type(NativeEventKind::REASONING), None);
        assert_eq!(native_event_mcf_type(NativeEventKind::UNKNOWN), None);
    }

    #[test]
    fn probe_version_parser_accepts_vendor_prefixes() {
        assert_eq!(
            (
                super::parse_version_from_probe("Hermes Agent 0.21.5")
                    .unwrap()
                    .major,
                super::parse_version_from_probe("Hermes Agent 0.21.5")
                    .unwrap()
                    .minor
            ),
            (0, 21)
        );
        assert_eq!(
            super::parse_version_from_probe("v1.18.34").unwrap().line(),
            "1.x"
        );
        assert_eq!(
            super::parse_version_from_probe("release 2.0.22-beta")
                .unwrap()
                .prerelease
                .as_deref(),
            Some("beta")
        );
        assert!(super::parse_version_from_probe("no version here").is_none());
    }

    #[test]
    fn any_adapter_routes_to_the_closed_agent_set() {
        assert_eq!(
            AnyAgentAdapter::for_agent(AgentType::Hermes).agent_type(),
            AgentType::Hermes
        );
        assert_eq!(
            AnyAgentAdapter::for_agent(AgentType::Kilo).agent_type(),
            AgentType::Kilo
        );
        assert_eq!(
            AnyAgentAdapter::for_agent(AgentType::OpenCode).agent_type(),
            AgentType::OpenCode
        );
    }

    #[test]
    fn contract_surface_probe_rejects_forbidden_remote_surface_but_ignores_prompt_text() {
        let definition = serde_json::json!({
            "remote_forbidden_flags": ["--share"],
            "remote_forbidden_commands": ["serve"]
        });
        assert!(super::launch_vector_has_no_forbidden_remote_surface(
            &definition,
            &[
                "run".into(),
                "mayasaba-capability-probe".into(),
                "--format".into(),
                "json".into()
            ],
            "mayasaba-capability-probe"
        ));
        assert!(!super::launch_vector_has_no_forbidden_remote_surface(
            &definition,
            &["run".into(), "--share".into()],
            "mayasaba-capability-probe"
        ));
        assert!(!super::launch_vector_has_no_forbidden_remote_surface(
            &definition,
            &["run".into(), "serve".into()],
            "mayasaba-capability-probe"
        ));
        assert!(super::launch_vector_has_no_forbidden_remote_surface(
            &definition,
            &["run".into(), "please serve the project".into()],
            "please serve the project"
        ));
    }

    #[test]
    fn contract_surface_probe_ignores_dynamic_placeholders_and_checks_flags() {
        let definition: serde_json::Value = serde_json::json!({
            "launch":["run","<prompt>","--format","json","--dir","<workspace>"],
            "resume":["run","<prompt>","--session","<native_session_id>","--format","json","--dir","<workspace>"]
        });
        assert!(super::contract_surface_present(
            &definition,
            "launch",
            "Usage: run <message>\n--format <format>\n--dir <path>\n"
        ));
        assert!(super::contract_surface_present(
            &definition,
            "resume",
            "Usage: run <message>\n--session <id>\n--format <format>\n--dir <path>\n"
        ));
        assert!(!super::contract_surface_present(
            &definition,
            "resume",
            "Usage: run <message>\n--format <format>\n--dir <path>\n"
        ));
    }

    #[test]
    fn discovery_options_have_a_bounded_default_probe_window() {
        let options = ProbeOptions::default();
        assert_eq!(options.timeout_ms, 10_000);
        assert!(options.timeout_ms >= 100);
    }

    #[test]
    fn normalization_preserves_unknown_records() {
        let event = OpenCodeAdapter
            .normalize_event(
                r#"{"type":"step_start","sessionID":"native-1","part":{}}"#,
                "2026-10-05T15:30:00Z",
                "artifact://raw/1",
            )
            .expect("json");
        assert_eq!(event.native_kind, NativeEventKind::STEP_START);
        assert_eq!(event.native_session_id.as_deref(), Some("native-1"));

        let unknown = OpenCodeAdapter
            .normalize_event(
                r#"{"type":"future","sessionID":"native-1"}"#,
                "2026-10-05T15:30:01Z",
                "artifact://raw/2",
            )
            .expect("unknown native record");
        assert_eq!(unknown.native_kind, NativeEventKind::UNKNOWN);
    }
}

// --- Agent performance reporting (DEC-112).
//
// Derived, read-only and informational. It exists because `get_agent_status` is a declared query with no
// implementation and because agent attempt data is recorded in `task_attempts` and summarised nowhere. It reports
// raw counts with a sample size per agent, and reports a rate only when the reporting policy's minimum sample is
// met, because a percentage over a handful of attempts reads as a measurement.
//
// Nothing here is read by task selection, routing, mode selection or any threshold. The policy that owns the
// suppression rule says so itself, and `crates/tasks/tests/selection.rs` proves selection output is byte-identical
// whether or not attempt data exists.

/// The reporting policy, embedded from its canonical owner rather than copied.
///
/// The block lives in `council-policies.json` because decision-class reporting is council-owned, and its rule
/// already covers reporting per agent. Reading a policy is not owning it; minting a second reporting policy for
/// agent telemetry would be a second source of truth for one rule (DEC-017), and copying the minimum sample into
/// a Rust constant is the "locked constant" this repository keeps out of code.
const COUNCIL_POLICIES: &str = include_str!("../../../schemas/council-v1/council-policies.json");

/// What could not be reported, so a caller never reads an absent value as a zero.
#[derive(Debug)]
pub enum AgentReportError {
    Policy(String),
    Storage(mayasaba_storage::StorageError),
}

impl From<mayasaba_storage::StorageError> for AgentReportError {
    fn from(value: mayasaba_storage::StorageError) -> Self {
        Self::Storage(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStateCount {
    pub state: String,
    pub count: i64,
}

/// One agent's attempt history in a project, with any rate the reporting policy permits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentPerformanceReport {
    pub agent_id: String,
    pub sample_size: i64,
    pub by_state: Vec<AgentStateCount>,
    pub attempts_with_recorded_failure: i64,
    /// Present only when `sample_size` reaches the policy's minimum. `None` is the suppression: a rate that is
    /// not reported is absent rather than zero, because zero is a measurement and this is the absence of one.
    pub failure_rate: Option<f64>,
    /// Why `failure_rate` is absent, so a reader is told the rate was withheld rather than that it was zero.
    pub failure_rate_suppressed_reason: Option<String>,
    /// Whether attempts could be tied to validation outcomes. Always `UNAVAILABLE` today: `validation_runs`
    /// carries `task_id` and no `attempt_id`, so no attempt can be attributed a validation result. Reported
    /// rather than estimated.
    pub validation_survival: String,
    pub validation_survival_reason: String,
    /// Stated on every report because the guarantee is the point of the report: this data is descriptive and
    /// must not affect routing, thresholds, mode selection or authority.
    pub informational_only: bool,
}

/// The minimum sample the reporting policy requires before a rate may be shown.
///
/// Read from the policy file rather than declared here, so the threshold has one owner.
pub fn minimum_sample_for_percentage() -> Result<i64, AgentReportError> {
    let parsed: Value = serde_json::from_str(COUNCIL_POLICIES).map_err(|error| {
        AgentReportError::Policy(format!(
            "schemas/council-v1/council-policies.json is not valid JSON: {error}"
        ))
    })?;
    parsed
        .get("reporting")
        .and_then(|reporting| reporting.get("minimum_sample_for_percentage"))
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            AgentReportError::Policy(
                "schemas/council-v1/council-policies.json declares no reporting.minimum_sample_for_percentage"
                    .to_string(),
            )
        })
}

/// The reason a rate is withheld, or absent when the sample is large enough to report one.
fn failure_rate(
    attempts: i64,
    failures: i64,
    minimum_sample: i64,
) -> (Option<f64>, Option<String>) {
    if attempts < minimum_sample {
        return (
            None,
            Some(format!(
                "sample size {attempts} is below the reporting policy minimum of {minimum_sample}; a rate over this many attempts would read as a measurement"
            )),
        );
    }
    if attempts == 0 {
        // Unreachable while the minimum is at least one, and stated rather than divided by, because a
        // zero-attempt agent must not produce a rate from nothing.
        return (
            None,
            Some("no attempts were recorded, so there is no rate to compute".to_string()),
        );
    }
    (Some(failures as f64 / attempts as f64), None)
}

impl AgentService {
    /// Agent performance reports for a project: raw counts, a sample size, and a rate only when the policy allows.
    ///
    /// This is a query over what was recorded. It writes nothing, it is not consulted by selection, and the
    /// attempt-to-validation link it would need to report validation survival does not exist, so that figure is
    /// reported `UNAVAILABLE` rather than inferred from task-level validation.
    pub fn agent_performance_report(
        &self,
        project_id: &str,
    ) -> Result<Vec<AgentPerformanceReport>, AgentReportError> {
        let minimum_sample = minimum_sample_for_percentage()?;
        let counts = self.storage.agent_attempt_counts(project_id)?;
        Ok(counts
            .into_iter()
            .map(|counts| {
                let (rate, suppressed) = failure_rate(
                    counts.sample_size,
                    counts.attempts_with_recorded_failure,
                    minimum_sample,
                );
                AgentPerformanceReport {
                    agent_id: counts.agent_id,
                    sample_size: counts.sample_size,
                    by_state: counts
                        .by_state
                        .into_iter()
                        .map(|state| AgentStateCount {
                            state: state.state,
                            count: state.count,
                        })
                        .collect(),
                    attempts_with_recorded_failure: counts.attempts_with_recorded_failure,
                    failure_rate: rate,
                    failure_rate_suppressed_reason: suppressed,
                    validation_survival: "UNAVAILABLE".to_string(),
                    validation_survival_reason:
                        "validation_runs records task_id and no attempt_id, so no attempt can be attributed a validation result; the figure is reported as unavailable rather than inferred from task-level validation"
                            .to_string(),
                    informational_only: true,
                }
            })
            .collect())
    }
}
