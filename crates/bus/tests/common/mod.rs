//! Fixtures shared by the bus's integration tests.
//!
//! `#![allow(dead_code)]` because each test binary compiles this module separately and uses a different part of
//! it; without it, the fixtures one binary does not touch are warnings in that binary.
#![allow(dead_code)]

use mayasaba_bus::Transport;
use mayasaba_bus::TransportError;
use mayasaba_storage::{NewProject, Storage};

/// A temporary directory for a project's `local_path`, removed and recreated so a rerun starts clean.
pub fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-bus-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

pub fn project(dir: &std::path::Path, tag: &str) -> NewProject {
    NewProject {
        project_id: format!("prj_{tag}"),
        local_path: dir.to_string_lossy().into_owned(),
        brief_id: format!("brf_{tag}"),
        brief_body: "Body".to_string(),
        brief_source: "TEST".to_string(),
        event_id: format!("evt_{tag}_genesis"),
        created_at: "2026-10-04T00:00:00Z".to_string(),
    }
}

/// A complete, legal MCF-v2 envelope, assembled by concatenation so the JSON braces stay readable.
///
/// `created_at` is a parameter because the dispatch tests need to control it: it is the anchor the outbox's
/// first due time is derived from.
pub fn envelope_at(
    message_id: &str,
    project_id: &str,
    sequence: i64,
    operation_id: Option<&str>,
    payload: &str,
    created_at: &str,
) -> String {
    let operation = match operation_id {
        Some(id) => format!(r#""{id}""#),
        None => "null".to_string(),
    };
    [
        r#"{"protocol_version":"MCF-2","schema_version":"2.0.0","#.to_string(),
        format!(r#""message_id":"{message_id}","event_id":"src_{message_id}","#),
        format!(r#""project_id":"{project_id}","session_id":"sess_1","#),
        r#""sender":{"actor_type":"MAYASABA","actor_id":"controller"},"#.to_string(),
        r#""recipients":[{"actor_type":"AGENT","actor_id":"hermes-1"}],"#.to_string(),
        r#""channel":"task","message_type":"TASK","phase":"IMPLEMENTATION","#.to_string(),
        format!(r#""correlation_id":"corr_1","sequence":{sequence},"project_epoch":0,"#),
        r#""priority":"TASK_CONTROL","#.to_string(),
        format!(r#""created_at":"{created_at}","#),
        r#""requires_ack":true,"requires_response":false,"blocking":false,"#.to_string(),
        format!(r#""payload":{payload},"#),
        r#""security":{"classification":"INTERNAL_PROJECT","secret_refs":[]},"#.to_string(),
        format!(r#""operation_id":{operation}}}"#),
    ]
    .concat()
}

pub fn envelope(
    message_id: &str,
    project_id: &str,
    sequence: i64,
    operation_id: Option<&str>,
    payload: &str,
) -> String {
    envelope_at(
        message_id,
        project_id,
        sequence,
        operation_id,
        payload,
        "2026-10-04T00:00:05Z",
    )
}

/// A storage handle holding one created project.
pub fn storage_with_project(tag: &str) -> (Storage, std::path::PathBuf) {
    let dir = existing_dir(tag);
    let mut storage = Storage::open_in_memory().expect("open");
    storage.create_project(&project(&dir, tag)).expect("create");
    (storage, dir)
}

/// A transport that answers from a script and remembers what it was handed.
///
/// Answers in order and then keeps answering with the last answer, so a test can say "fail twice then succeed"
/// without enumerating every attempt.
pub struct ScriptedTransport {
    script: Vec<Result<(), TransportError>>,
    cursor: usize,
    /// Every envelope handed over, in order, including the ones that were refused.
    pub sent: Vec<String>,
}

impl ScriptedTransport {
    pub fn always_ok() -> Self {
        ScriptedTransport {
            script: vec![Ok(())],
            cursor: 0,
            sent: Vec::new(),
        }
    }

    pub fn scripted(script: Vec<Result<(), TransportError>>) -> Self {
        assert!(!script.is_empty(), "a script needs at least one answer");
        ScriptedTransport {
            script,
            cursor: 0,
            sent: Vec::new(),
        }
    }

    pub fn always(err: TransportError) -> Self {
        ScriptedTransport {
            script: vec![Err(err)],
            cursor: 0,
            sent: Vec::new(),
        }
    }

    pub fn attempts(&self) -> usize {
        self.sent.len()
    }
}

impl Transport for ScriptedTransport {
    fn send(&mut self, envelope_json: &str) -> Result<(), TransportError> {
        self.sent.push(envelope_json.to_string());
        let answer = self
            .script
            .get(self.cursor)
            .cloned()
            .unwrap_or_else(|| self.script[self.script.len() - 1].clone());
        if self.cursor + 1 < self.script.len() {
            self.cursor += 1;
        }
        answer
    }
}
