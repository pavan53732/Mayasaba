#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Mayasaba desktop shell.
//!
//! The shell is transport, not domain logic. It exposes typed Tauri commands that delegate to an owning
//! application service and return authoritative state; it never decides anything itself (AGENTS.md
//! section 11, DEC-026). `create_project` is the first command wired end to end.

use std::sync::Mutex;

use mayasaba_core::project_service::{CreateProjectRequest, ProjectService, ProjectValidationError};
use mayasaba_workspace::WorkspaceRejection;
use serde::Serialize;
use tauri::State;

/// Durable database location. App data, not the workspace (DEC-020).
fn database_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::Path::new(&base).join("Mayasaba").join("mayasaba.sqlite3")
}

/// The authoritative project, as the Control Room must display it. This is a projection of stored state,
/// never UI-side draft state.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectView {
    project_id: String,
    name: String,
    local_path: String,
    phase: String,
    status: String,
    current_epoch: i64,
    brief_id: Option<String>,
    brief_version: Option<i64>,
    brief_body: Option<String>,
}

/// A rejection carrying a machine-readable reason, so the Control Room can distinguish a validation
/// failure from a storage failure without parsing prose.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: &'static str,
    message: String,
}

impl From<ProjectValidationError> for CommandError {
    fn from(e: ProjectValidationError) -> Self {
        let code = match e {
            ProjectValidationError::EmptyField(_) => "EMPTY_FIELD",
            ProjectValidationError::BlankInitialBrief => "BLANK_INITIAL_BRIEF",
            ProjectValidationError::IntentNotYetAssessed => "INTAKE_NOT_IMPLEMENTED",
        };
        CommandError { code, message: e.to_string() }
    }
}

/// The outcome of validating a workspace candidate.
///
/// `AUTHORIZED` means the folder exists, is local, and is a directory Rust could canonicalize. It does not
/// mean every operation inside it is permitted: task-scoped paths, policy and leases remain narrower
/// (DEC-048).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCheck {
    status: &'static str,
    canonical_path: Option<String>,
    requested_path: String,
    code: Option<String>,
    message: Option<String>,
}

#[tauri::command]
fn validate_workspace(path: String) -> WorkspaceCheck {
    // Selection is not authorization, so this is a real check rather than an echo. A path that does not exist
    // must never come back AUTHORIZED, and nothing is created to make it pass.
    match mayasaba_core::project_service::validate_workspace(&path) {
        Ok(ok) => WorkspaceCheck {
            status: "AUTHORIZED",
            canonical_path: Some(ok.canonical_path),
            requested_path: ok.requested_path,
            code: None,
            message: None,
        },
        Err(rejection) => WorkspaceCheck {
            status: "INVALID",
            canonical_path: None,
            requested_path: path,
            code: Some(rejection.code().to_string()),
            message: Some(rejection.to_string()),
        },
    }
}

#[tauri::command]
fn create_project(
    service: State<'_, Mutex<ProjectService>>,
    name: String,
    local_path: String,
    initial_brief: String,
) -> Result<ProjectView, CommandError> {
    let request = CreateProjectRequest {
        name,
        local_path,
        initial_brief_body: initial_brief,
        brief_source: None,
    };

    let mut service = service.lock().map_err(|_| CommandError {
        code: "SERVICE_POISONED",
        message: "ProjectService lock was poisoned by a prior panic".to_string(),
    })?;

    let outcome = service.create_project(&request).map_err(|e| match e {
        mayasaba_core::project_service::CreateProjectError::Validation(v) => v.into(),
        mayasaba_core::project_service::CreateProjectError::Workspace(WorkspaceRejection::Empty) => CommandError {
            code: "EMPTY_FIELD",
            message: WorkspaceRejection::Empty.to_string(),
        },
        mayasaba_core::project_service::CreateProjectError::Workspace(w) => CommandError {
            code: w.code(),
            message: w.to_string(),
        },
        other => CommandError { code: "STORAGE_FAILURE", message: other.to_string() },
    })?;

    let project = match outcome {
        mayasaba_core::project_service::CreateProjectOutcome::Created { project } => project,
    };

    Ok(ProjectView {
        project_id: project.project_id,
        name: project.name,
        local_path: project.local_path,
        phase: project.phase,
        status: project.status,
        current_epoch: project.current_epoch,
        brief_id: project.brief_id,
        brief_version: project.brief_version,
        brief_body: project.brief_body,
    })
}

fn main() {
    // The service owns its store and is registered as managed state. It is created eagerly so a failure to
    // open the durable store surfaces at startup rather than on the first command.
    let service = ProjectService::open(&database_path())
        .expect("Mayasaba could not open its durable store; see AGENTS.md section 20");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(service))
        .invoke_handler(tauri::generate_handler![create_project, validate_workspace])
        .run(tauri::generate_context!())
        .expect("error while running Mayasaba");
}