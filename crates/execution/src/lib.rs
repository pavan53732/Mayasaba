//! Mayasaba local execution boundary.
//!
//! Material process creation is owned here. Callers must first persist an execution in the canonical storage
//! model; this module then re-checks the task-attempt fence immediately before the process crosses the OS boundary.
//! Adapter-specific process policy arrives as a generic ProcessSpec so this crate does not depend on the agent crate.

use mayasaba_storage::{
    NewProcessRecord, Result as StorageResult, Storage, StorageError,
};
use std::{
    collections::BTreeMap,
    fmt,
    process::Stdio,
    time::Duration,
};

mod process_tree;
use process_tree::ProcessTreeOwner;

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time,
};

pub const CRATE_NAME: &str = "mayasaba-execution";
const DEFAULT_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAuthorization {
    pub attempt_id: String,
    pub lease_version: i64,
}

/// Admission hook required immediately before a material execution starts.
pub fn authorize_execution(
    storage: &Storage,
    attempt_id: &str,
    lease_version: i64,
) -> StorageResult<ExecutionAuthorization> {
    storage.verify_attempt_authority(attempt_id, lease_version)?;
    Ok(ExecutionAuthorization {
        attempt_id: attempt_id.to_owned(),
        lease_version,
    })
}

/// The process-neutral output produced by an agent adapter. The execution kernel is the only owner of spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub executable: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub environment: BTreeMap<String, String>,
    pub scrub_inherited_environment: Vec<String>,
}

impl ProcessSpec {
    pub fn new(
        executable: impl Into<String>,
        argv: Vec<String>,
        cwd: impl Into<String>,
    ) -> Self {
        Self {
            executable: executable.into(),
            argv,
            cwd: cwd.into(),
            environment: BTreeMap::new(),
            scrub_inherited_environment: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct SpawnedProcess {
    child: Child,
    stdout: Option<tokio::io::BufReader<tokio::process::ChildStdout>>,
    stderr: Option<tokio::io::BufReader<tokio::process::ChildStderr>>,
    pub execution_id: String,
    pub pid: u32,
    pub output_limit_bytes: usize,
    process_tree: ProcessTreeOwner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedProcess {
    pub execution_id: String,
    pub pid: u32,
    pub exit_code: Option<i64>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
}

#[derive(Debug)]
pub enum ExecutionError {
    Storage(StorageError),
    Spawn(std::io::Error),
    OutputJoin(String),
    InvalidSpec(String),
    Termination(String),
    Durability(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(err) => write!(f, "storage error: {err}"),
            Self::Spawn(err) => write!(f, "process spawn failed: {err}"),
            Self::OutputJoin(err) => write!(f, "output reader failed: {err}"),
            Self::InvalidSpec(err) => write!(f, "invalid process specification: {err}"),
            Self::Termination(err) => write!(f, "owned-process termination failed: {err}"),
            Self::Durability(err) => write!(f, "post-side-effect durability failure: {err}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<StorageError> for ExecutionError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

/// Spawn a process only after its durable execution row is APPROVED and, when task-bound, its fence is current.
///
/// Durable-before-side-effect ordering is deliberate:
/// APPROVED -> STARTING is committed before spawn. After spawn, the PID is persisted and STARTING -> RUNNING is
/// committed. If a post-spawn persistence step fails, the kernel attempts owned-tree termination rather than
/// allowing an unrecorded process to run unattended.
pub async fn spawn_process(
    storage: &Storage,
    execution_id: &str,
    attempt_id: Option<&str>,
    lease_version: Option<i64>,
    spec: &ProcessSpec,
    output_limit_bytes: Option<usize>,
    started_at: &str,
) -> Result<SpawnedProcess, ExecutionError> {
    let execution = storage
        .get_command_execution(execution_id)?
        .ok_or_else(|| ExecutionError::Durability(format!("command execution {execution_id} does not exist")))?;

    if let Some(attempt_id) = attempt_id {
        let fence = lease_version.ok_or_else(|| {
            ExecutionError::InvalidSpec("attempt-bound execution requires lease_version".to_string())
        })?;
        authorize_execution(storage, attempt_id, fence)?;
        if execution.attempt_id.as_deref() != Some(attempt_id) {
            return Err(ExecutionError::InvalidSpec(
                "attempt_id does not match the persisted command execution".to_string(),
            ));
        }
    } else if lease_version.is_some() {
        return Err(ExecutionError::InvalidSpec(
            "lease_version cannot be supplied without attempt_id".to_string(),
        ));
    }

    if execution.status != "APPROVED" {
        return Err(ExecutionError::InvalidSpec(format!(
            "execution must be APPROVED before spawn; found {}",
            execution.status
        )));
    }
    if execution.executable != spec.executable || execution.cwd != spec.cwd {
        return Err(ExecutionError::InvalidSpec(
            "process spec does not match persisted execution executable/cwd".to_string(),
        ));
    }
    let persisted_args: Vec<String> = serde_json::from_str(&execution.arguments_json)
        .map_err(|e| ExecutionError::InvalidSpec(format!("persisted arguments are invalid JSON: {e}")))?;
    if persisted_args != spec.argv {
        return Err(ExecutionError::InvalidSpec(
            "process spec argv does not match the durably approved execution arguments".to_string(),
        ));
    }
    if spec.executable.trim().is_empty() || spec.cwd.trim().is_empty() {
        return Err(ExecutionError::InvalidSpec(
            "executable and cwd must be non-empty".to_string(),
        ));
    }

    storage.transition_command_execution(
        execution_id,
        "APPROVED",
        "STARTING",
        None,
        Some(started_at),
    )?;

    let mut command = Command::new(&spec.executable);
    command.args(&spec.argv).current_dir(&spec.cwd);
    process_tree::configure_suspended_creation(&mut command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    for key in &spec.scrub_inherited_environment {
        command.env_remove(key);
    }
    for (key, value) in &spec.environment {
        command.env(key, value);
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            // STARTING intentionally remains nonterminal: no process crossed the OS boundary, so CRASHED would
            // fabricate a physical event. Recovery reconciles dangling STARTING rows as a launch failure.
            return Err(ExecutionError::Spawn(err));
        }
    };

    let pid = match child.id() {
        Some(pid) => pid,
        None => {
            let mut child = child;
            let _ = child.kill().await;
            return Err(ExecutionError::Durability(
                "spawned process did not expose a PID and was terminated before supervision".to_string(),
            ));
        }
    };

    // On Windows ownership is established before any child code executes: suspended spawn -> Job Object -> resume.
    let mut process_tree = match ProcessTreeOwner::attach_and_resume(&mut child, pid).await {
        Ok(owner) => owner,
        Err(err) => {
            return Err(ExecutionError::Termination(format!(
                "process containment setup failed for pid {pid}: {err}"
            )));
        }
    };

    let process_record_id = format!("proc_{execution_id}_{pid}");
    if let Err(err) = storage.insert_process_record(&NewProcessRecord {
        process_record_id,
        execution_id: execution_id.to_owned(),
        pid: i64::from(pid),
        parent_pid: std::process::id().try_into().ok(),
        state: "EXPECTED".to_owned(),
        observed_at: started_at.to_owned(),
    }) {
        let termination = process_tree.terminate().await;
        return match termination {
            Ok(()) => Err(ExecutionError::Durability(format!(
                "PID persistence failed after spawn; owned process tree was terminated: {err}"
            ))),
            Err(term_err) => Err(ExecutionError::Termination(format!(
                "PID persistence failed after spawn and owned process tree could not be terminated: {err}; {term_err}"
            ))),
        };
    }

    if let Err(err) = storage.transition_command_execution(
        execution_id,
        "STARTING",
        "RUNNING",
        None,
        None,
    ) {
        let termination = terminate_owned_tree(pid).await;
        return match termination {
            Ok(()) => Err(ExecutionError::Durability(format!(
                "STARTING -> RUNNING persistence failed after spawn; owned process tree was terminated: {err}"
            ))),
            Err(term_err) => Err(ExecutionError::Termination(format!(
                "STARTING -> RUNNING persistence failed after spawn and process tree could not be terminated: {err}; {term_err}"
            ))),
        };
    }

    Ok(SpawnedProcess {
        stdout: child.stdout.take().map(tokio::io::BufReader::new),
        stderr: child.stderr.take().map(tokio::io::BufReader::new),
        child,
        execution_id: execution_id.to_owned(),
        pid,
        output_limit_bytes: output_limit_bytes.unwrap_or(DEFAULT_OUTPUT_LIMIT),
    })
}

impl SpawnedProcess {
    /// Terminate the owned process tree. Callers use this when a post-spawn controller transaction fails.
    pub async fn terminate(&mut self) -> Result<(), ExecutionError> {
        self.process_tree
            .terminate()
            .await
            .map_err(ExecutionError::Termination)
    }

    /// Read one stdout line without waiting for the process to exit. This is the primitive used by the agent
    /// gateway to stream structured JSONL events during long-running sessions.
    pub async fn next_stdout_line(&mut self) -> Result<Option<String>, ExecutionError> {
        use tokio::io::AsyncBufReadExt;
        let Some(stdout) = self.stdout.as_mut() else {
            return Ok(None);
        };
        let mut line = String::new();
        let read = stdout.read_line(&mut line).await.map_err(ExecutionError::Spawn)?;
        if read == 0 { Ok(None) } else { Ok(Some(line)) }
    }

    /// Read one stderr line for diagnostics/session identity. stderr is deliberately separate from structured
    /// stdout so vendor diagnostics cannot be mistaken for canonical protocol records.
    pub async fn next_stderr_line(&mut self) -> Result<Option<String>, ExecutionError> {
        use tokio::io::AsyncBufReadExt;
        let Some(stderr) = self.stderr.as_mut() else {
            return Ok(None);
        };
        let mut line = String::new();
        let read = stderr.read_line(&mut line).await.map_err(ExecutionError::Spawn)?;
        if read == 0 { Ok(None) } else { Ok(Some(line)) }
    }

    /// Wait until the process terminates. stdout/stderr are drained concurrently to avoid pipe back-pressure
    /// deadlocking a long-running coding agent. Output is bounded and truncation is explicit.
    pub async fn wait(mut self, storage: &Storage, observed_at: &str) -> Result<CompletedProcess, ExecutionError> {
        self.finish_wait(storage, observed_at, None).await
    }

    /// Wait with a hard deadline. On timeout Windows uses taskkill /T so owned descendants are targeted; if the
    /// tree cannot be terminated/observed, the durable execution is intentionally left nonterminal for recovery.
    pub async fn wait_timeout(
        mut self,
        storage: &Storage,
        observed_at: &str,
        timeout: Duration,
    ) -> Result<CompletedProcess, ExecutionError> {
        self.finish_wait(storage, observed_at, Some(timeout)).await
    }

    async fn finish_wait(
        &mut self,
        storage: &Storage,
        observed_at: &str,
        timeout: Option<Duration>,
    ) -> Result<CompletedProcess, ExecutionError> {
        let stdout = self.stdout.take();
        let stderr = self.stderr.take();
        let limit = self.output_limit_bytes;
        let stdout_task = tokio::spawn(read_bounded(stdout, limit));
        let stderr_task = tokio::spawn(read_bounded(stderr, limit));

        let mut timed_out = false;
        let status = match timeout {
            Some(limit_duration) => match time::timeout(limit_duration, self.child.wait()).await {
                Ok(result) => result.map_err(ExecutionError::Spawn)?,
                Err(_) => {
                    timed_out = true;
                    if let Err(err) = self.process_tree.terminate().await {
                        insert_unknown_observation(storage, &self.execution_id, self.pid, observed_at).ok();
                        return Err(ExecutionError::Termination(err));
                    }
                    self.child.wait().await.map_err(ExecutionError::Spawn)?
                }
            },
            None => self.child.wait().await.map_err(ExecutionError::Spawn)?,
        };

        let (stdout, stdout_truncated) = stdout_task
            .await
            .map_err(|e| ExecutionError::OutputJoin(e.to_string()))?
            .map_err(ExecutionError::Spawn)?;
        let (stderr, stderr_truncated) = stderr_task
            .await
            .map_err(|e| ExecutionError::OutputJoin(e.to_string()))?
            .map_err(ExecutionError::Spawn)?;

        let exit_code = status.code().map(i64::from);
        let observation_state = if timed_out { "VERIFIED" } else { "VERIFIED" };
        let _ = storage.insert_process_record(&NewProcessRecord {
            process_record_id: format!("proc_{}_{}_{}", self.execution_id, self.pid, observed_at.replace(':', "_")),
            execution_id: self.execution_id.clone(),
            pid: i64::from(self.pid),
            parent_pid: Some(std::process::id().into()),
            state: observation_state.to_owned(),
            observed_at: observed_at.to_owned(),
        });

        let next_state = if timed_out { "TIMEOUT" } else { "EXITED" };
        storage.transition_command_execution(
            &self.execution_id,
            "RUNNING",
            next_state,
            exit_code,
            Some(observed_at),
        )?;

        Ok(CompletedProcess {
            execution_id: self.execution_id.clone(),
            pid: self.pid,
            exit_code,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
        })
    }
}

async fn read_bounded<R: AsyncRead + Unpin>(
    reader: Option<R>,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let Some(mut reader) = reader else {
        return Ok((Vec::new(), false));
    };
    let mut bytes = Vec::new();
    reader.take(limit.saturating_add(1) as u64).read_to_end(&mut bytes).await?;
    let truncated = bytes.len() > limit;
    if truncated {
        bytes.truncate(limit);
    }
    Ok((bytes, truncated))
}

fn insert_unknown_observation(
    storage: &Storage,
    execution_id: &str,
    pid: u32,
    observed_at: &str,
) -> StorageResult<()> {
    storage.insert_process_record(&NewProcessRecord {
        process_record_id: format!("proc_{}_{}_unknown", execution_id, pid),
        execution_id: execution_id.to_owned(),
        pid: i64::from(pid),
        parent_pid: Some(std::process::id().into()),
        state: "UNKNOWN".to_owned(),
        observed_at: observed_at.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_spec_keeps_explicit_environment_boundaries() {
        let mut spec = ProcessSpec::new("hermes", vec!["chat".into()], r"C:\Work\Mayasaba");
        spec.environment.insert("PWD".into(), r"C:\Work\Mayasaba".into());
        spec.scrub_inherited_environment.push("HERMES_YOLO_MODE".into());
        assert_eq!(spec.environment.get("PWD").map(String::as_str), Some(r"C:\Work\Mayasaba"));
        assert!(spec.scrub_inherited_environment.contains(&"HERMES_YOLO_MODE".into()));
    }
}
