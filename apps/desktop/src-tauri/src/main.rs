#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Mayasaba desktop shell.
//!
//! The shell is transport, not domain logic. It exposes typed Tauri commands that delegate to an owning
//! application service and return authoritative state; it never decides anything itself (AGENTS.md
//! section 11, DEC-026). `create_project` is the first command wired end to end.
//!
//! The wire format is **snake_case**, spelling every field exactly as `schemas/tauri-bridge-v1/payload-types.json`
//! declares it - `local_path`, `canonical_path`, `integrity_ok` - so the contract and the bytes on the wire are
//! the same names and the frontend has nothing to translate (DEC-054). Both halves are stated explicitly rather
//! than left to a default: `#[serde(rename_all = "snake_case")]` on every wire struct and
//! `#[tauri::command(rename_all = "snake_case")]` on every command, because Tauri's default for command
//! arguments is camelCase and an implicit default is how the two sides drifted apart in the first place.

mod bus_shell;

use std::sync::Mutex;

use mayasaba_core::bus_runtime::{CoreClock, CoreIdSource};
use mayasaba_core::diagnostics::DiagnosticsService;
use mayasaba_core::project_service::{
    CreateProjectRequest, ProjectService, ProjectValidationError,
};
use mayasaba_core::recovery_service::{RecoveryService, ReplayRefusal};
use serde::Serialize;
use tauri::State;

/// Durable database location. App data, not the workspace (DEC-020).
fn database_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::Path::new(&base)
        .join("Mayasaba")
        .join("mayasaba.sqlite3")
}

/// The authoritative project, as the Control Room must display it. This is a projection of stored state,
/// never UI-side draft state.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
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
    created_at: String,
}

impl From<mayasaba_storage::ProjectRecord> for ProjectView {
    /// One conversion, so a listed project and a created project cannot drift apart in shape.
    fn from(p: mayasaba_storage::ProjectRecord) -> Self {
        ProjectView {
            project_id: p.project_id,
            name: p.name,
            local_path: p.local_path,
            phase: p.phase,
            status: p.status,
            current_epoch: p.current_epoch,
            brief_id: p.brief_id,
            brief_version: p.brief_version,
            brief_body: p.brief_body,
            created_at: p.created_at,
        }
    }
}

/// A rejection carrying a machine-readable reason, so the Control Room can distinguish a validation
/// failure from a storage failure without parsing prose.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
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
        CommandError {
            code,
            message: e.to_string(),
        }
    }
}

/// The bus's own error codes, reused rather than re-listed.
///
/// This delegates to `BusError::code()` instead of matching each variant. A second match here would be a second
/// source of truth for the same fact, and the two could disagree - which is exactly the divergence DEC-055
/// exists to prevent. The wire therefore carries the bus's registry key unchanged, and the gate already scans
/// that mapping in `crates/bus/src/error.rs`.
impl From<mayasaba_bus::BusError> for CommandError {
    fn from(error: mayasaba_bus::BusError) -> Self {
        CommandError {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

/// The outcome of validating a workspace candidate.
///
/// `AUTHORIZED` means the folder exists, is local, and is a directory Rust could canonicalize. It does not
/// mean every operation inside it is permitted: task-scoped paths, policy and leases remain narrower
/// (DEC-048).
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct WorkspaceCheck {
    status: &'static str,
    canonical_path: Option<String>,
    requested_path: String,
    /// Derived here so the Control Room displays the same value that `create_project` will persist. If React
    /// computed the name itself, the preview and the stored value would be two derivations that can drift.
    derived_project_name: Option<String>,
    code: Option<String>,
    message: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
fn validate_workspace(path: String) -> WorkspaceCheck {
    // Selection is not authorization, so this is a real check rather than an echo. A path that does not exist
    // must never come back AUTHORIZED, and nothing is created to make it pass.
    match mayasaba_core::project_service::validate_workspace(&path) {
        Ok(ok) => WorkspaceCheck {
            status: "AUTHORIZED",
            canonical_path: Some(ok.canonical_path),
            derived_project_name: Some(ok.derived_project_name),
            requested_path: ok.requested_path,
            code: None,
            message: None,
        },
        Err(rejection) => WorkspaceCheck {
            status: "INVALID",
            canonical_path: None,
            derived_project_name: None,
            requested_path: path,
            code: Some(rejection.code().to_string()),
            message: Some(rejection.to_string()),
        },
    }
}

/// The result of the startup recovery scan, surfaced so a damaged database is visible rather than silent.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct RecoveryView {
    clean: bool,
    integrity_ok: bool,
    issues: Vec<RecoveryIssueView>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct RecoveryIssueView {
    kind: String,
    detail: String,
}

#[tauri::command(rename_all = "snake_case")]
fn get_recovery_status(
    service: State<'_, Mutex<ProjectService>>,
) -> Result<RecoveryView, CommandError> {
    let service = service.lock().map_err(|_| CommandError {
        code: "SERVICE_POISONED",
        message: "ProjectService lock was poisoned by a prior panic".to_string(),
    })?;
    let report = service.recover().map_err(|e| CommandError {
        code: "STORAGE_FAILURE",
        message: e.to_string(),
    })?;
    Ok(RecoveryView {
        clean: report.is_clean(),
        integrity_ok: report.integrity_ok,
        issues: report
            .issues
            .iter()
            .map(|i| RecoveryIssueView {
                kind: i.kind.to_string(),
                detail: i.detail.clone(),
            })
            .collect(),
    })
}

#[tauri::command(rename_all = "snake_case")]
fn list_projects(
    service: State<'_, Mutex<ProjectService>>,
) -> Result<Vec<ProjectView>, CommandError> {
    // The rehydration path. Everything the Control Room shows for an existing project comes from here, so it
    // is the same authoritative projection creation returns rather than a UI-side reconstruction.
    let service = service.lock().map_err(|_| CommandError {
        code: "SERVICE_POISONED",
        message: "ProjectService lock was poisoned by a prior panic".to_string(),
    })?;
    let projects = service.list_projects().map_err(|e| CommandError {
        code: "STORAGE_FAILURE",
        message: e.to_string(),
    })?;
    Ok(projects.into_iter().map(ProjectView::from).collect())
}

#[tauri::command(rename_all = "snake_case")]
fn create_project(
    service: State<'_, Mutex<ProjectService>>,
    local_path: String,
    initial_brief: String,
) -> Result<ProjectView, CommandError> {
    // No `name` argument. The display name is derived from the validated canonical workspace path by the
    // owning service, so the caller cannot supply identity metadata that disagrees with the filesystem
    // (DEC-050).
    let request = CreateProjectRequest {
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
        // Every workspace rejection reports its own code, including `Empty`. There is deliberately no special
        // case for `WorkspaceRejection::Empty` mapping to `EMPTY_FIELD`: `ProjectService::validate` already
        // refuses a blank `local_path` with `EMPTY_FIELD` before workspace validation runs, so that arm was
        // unreachable and its only effect was to give one code two meanings - "a required request field was
        // blank" and "the workspace candidate was blank" - which no caller could tell apart (DEC-055).
        mayasaba_core::project_service::CreateProjectError::Workspace(w) => CommandError {
            code: w.code(),
            message: w.to_string(),
        },
        other => CommandError {
            code: "STORAGE_FAILURE",
            message: other.to_string(),
        },
    })?;

    let project = match outcome {
        mayasaba_core::project_service::CreateProjectOutcome::Created { project } => project,
    };

    Ok(ProjectView::from(project))
}

/// One derived sequence gap, carrying the registry code in the value.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct OpenGapView {
    code: String,
    session_id: String,
    channel: String,
    expected: i64,
    found: i64,
}

/// How many of a project's messages are unfinished, by state.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct DeliveryCountsView {
    queued: i64,
    retrying: i64,
    expired: i64,
    dead_lettered: i64,
}

/// The communication health of one project.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct CommunicationHealthView {
    project_id: String,
    backlog: i64,
    enqueue_limit: i64,
    counts: DeliveryCountsView,
    open_gaps: Vec<OpenGapView>,
    transport_status: String,
}

/// A consumer's durable position in a project's event stream.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct EventCursorView {
    project_id: String,
    consumer_id: String,
    last_sequence: i64,
    next_sequence: Option<i64>,
    gap_detected: bool,
    resync_from: Option<i64>,
}

/// What a completed dead-letter replay produced.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct ReplayDeadLetterView {
    replayed_message_id: String,
    source_message_id: String,
    context_snapshot_id: Option<String>,
    state_digest: Option<String>,
    context_refreshed: bool,
    deduplicated: bool,
}

/// Turn a replay refusal into the wire error the registry declares for it.
///
/// The code is chosen here rather than in `crates/core` so the registry stays the one place a code's meaning is
/// decided, and so every emission site stays visible to the gate that checks each emitted code is registered.
fn replay_refused(error: ReplayRefusal) -> CommandError {
    match error {
        // The bus chose the code for its own refusals, so it is asked rather than re-listed. A second list
        // would be a second source of truth for one fact (AGENTS.md section 6).
        ReplayRefusal::Bus(bus_error) => bus_error.into(),
        // Not UNAUTHORIZED and not POLICY_DENIED: no rule ran and nothing established that the actor is not
        // allowed, so either would report a judgement that was never made. This reports the absence instead.
        ReplayRefusal::AuthorizationNotImplemented {
            message_id,
            message_type,
        } => CommandError {
            code: "AUTHORIZATION_NOT_IMPLEMENTED",
            message: format!(
                "{message_id} is a {message_type}, which is a material action, and replaying a material action \
                 requires an authorization judgement that PolicyService does not make yet"
            ),
        },
        // The code the bus's own classifier gives a named row that does not exist, applied rather than
        // re-decided: a missing message is that condition reached from the other side, and it is not a storage
        // failure, because nothing went wrong with the database.
        ReplayRefusal::Unreadable { message_id, detail } => CommandError {
            code: "SCHEMA_INVALID",
            message: format!("{message_id} could not be read as a replayable message: {detail}"),
        },
    }
}

/// A poisoned lock and a panicked blocking task are both "this service is unusable", which is the one code the
/// registry has for that condition.
fn service_unusable(detail: String) -> CommandError {
    CommandError {
        code: "SERVICE_POISONED",
        message: detail,
    }
}

/// Run bus work off the delivery thread.
///
/// A synchronous `#[tauri::command]` runs inline on the IPC delivery thread (DEC-072), and every bus query is
/// SQLite work, so a handler that ran one inline would block that thread for the duration. The bus is reached
/// only through this helper so no handler can quietly reintroduce the blocking call.
async fn on_bus<T, F>(bus: bus_shell::SharedBus, work: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce(&mut bus_shell::BusShell) -> Result<T, CommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = bus.lock().map_err(|_| {
            service_unusable("the bus lock was poisoned by a prior panic".to_string())
        })?;
        work(&mut guard)
    })
    .await
    .map_err(|error| service_unusable(format!("the bus task did not finish: {error}")))?
}

/// Read-only. Reports the durable communication state of one project and never mutates it.
#[tauri::command(rename_all = "snake_case")]
async fn get_communication_health(
    bus: State<'_, bus_shell::SharedBus>,
    project_id: String,
) -> Result<CommunicationHealthView, CommandError> {
    let bus = bus.inner().clone();
    on_bus(bus, move |shell| {
        let health = DiagnosticsService::communication_health(shell.bus(), &project_id)?;
        Ok(CommunicationHealthView {
            project_id: health.project_id,
            backlog: health.backlog,
            enqueue_limit: health.enqueue_limit,
            counts: DeliveryCountsView {
                queued: health.counts.queued,
                retrying: health.counts.retrying,
                expired: health.counts.expired,
                dead_lettered: health.counts.dead_lettered,
            },
            open_gaps: health
                .open_gaps
                .into_iter()
                .map(|gap| OpenGapView {
                    code: gap.code.to_string(),
                    session_id: gap.session_id,
                    channel: gap.channel,
                    expected: gap.expected,
                    found: gap.found,
                })
                .collect(),
            transport_status: health.transport_status.to_string(),
        })
    })
    .await
}

/// Read-only. Reports a consumer's durable event position and never advances it.
#[tauri::command(rename_all = "snake_case")]
async fn get_event_cursor(
    bus: State<'_, bus_shell::SharedBus>,
    project_id: String,
    consumer_id: String,
) -> Result<EventCursorView, CommandError> {
    let bus = bus.inner().clone();
    on_bus(bus, move |shell| {
        let cursor =
            DiagnosticsService::event_cursor(shell.bus().storage(), &project_id, &consumer_id)?;
        Ok(EventCursorView {
            project_id: cursor.project_id,
            consumer_id: cursor.consumer_id,
            last_sequence: cursor.last_sequence,
            next_sequence: cursor.next_sequence,
            gap_detected: cursor.gap_detected,
            resync_from: cursor.resync_from,
        })
    })
    .await
}

/// Re-enqueue a dead-lettered message as a new one, refusing the types whose replay needs an authorizer.
///
/// The refusal is the operation's substance. Replay is the one path that would reach a material action without
/// passing the authorization every other material mutation passes, so it stays closed until `PolicyService`
/// exists (DEC-071). This does not honour `cancellable`, which the contract declares on this command and defines
/// nowhere; nothing here implies it does.
#[tauri::command(rename_all = "snake_case")]
async fn replay_dead_letter(
    bus: State<'_, bus_shell::SharedBus>,
    message_id: String,
) -> Result<ReplayDeadLetterView, CommandError> {
    let bus = bus.inner().clone();
    on_bus(bus, move |shell| {
        let outcome = RecoveryService::replay_dead_letter(
            shell.bus_mut(),
            &CoreIdSource,
            &CoreClock,
            &message_id,
        )
        .map_err(replay_refused)?;
        Ok(ReplayDeadLetterView {
            replayed_message_id: outcome.replayed_message_id,
            source_message_id: outcome.source_message_id,
            context_snapshot_id: outcome.context_snapshot_id,
            state_digest: outcome.state_digest,
            context_refreshed: outcome.context_refreshed,
            deduplicated: outcome.deduplicated,
        })
    })
    .await
}
fn main() {
    // The service owns its store and is registered as managed state. It is created eagerly so a failure to
    // open the durable store surfaces at startup rather than on the first command.
    let service = ProjectService::open(&database_path())
        .expect("Mayasaba could not open its durable store; see AGENTS.md section 20");

    // Recovery runs before the window can display anything, so a damaged database is reported rather than
    // rendered as a plausible-looking empty Control Room. It never repairs.
    match service.recover() {
        Ok(report) if report.is_clean() => {}
        Ok(report) => eprintln!(
            "mayasaba: startup recovery found {} issue(s):",
            report.issues.len()
        ),
        Err(error) => eprintln!("mayasaba: startup recovery could not run: {error}"),
    }

    // Its own connection, opened eagerly so a failure to reach the durable store surfaces at startup rather
    // than on the first command, exactly as the project service does above.
    let bus_state: bus_shell::SharedBus = std::sync::Arc::new(Mutex::new(
        bus_shell::BusShell::open(&database_path())
            .expect("Mayasaba could not open its durable bus store; see AGENTS.md section 20"),
    ));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(service))
        // The bus holds its own connection to the same store, never ProjectService's (DEC-072). It is managed
        // as an Arc<Mutex<..>> because a Tauri State borrow is not 'static and cannot move into the
        // spawn_blocking task a handler will use.
        .manage(bus_state)
        .invoke_handler(tauri::generate_handler![
            create_project,
            list_projects,
            get_recovery_status,
            validate_workspace,
            replay_dead_letter,
            get_communication_health,
            get_event_cursor
        ])
        .run(tauri::generate_context!())
        .expect("error while running Mayasaba");
}

/// Conformance between the serialized wire structs and the payload types the contract declares for them.
///
/// The gate proves that operation **names** agree between the contract and both sides of the bridge. It never
/// proved that a handler's *shape* agrees with the type the contract declares for it, and that gap is not
/// hypothetical: the wire renamed every multi-word field to camelCase while `payload-types.json` declared
/// snake_case, and nothing noticed until it was measured by hand (DEC-053, fixed by DEC-054). A test that
/// serializes the real struct and validates it against the real declaration closes that class of divergence,
/// because it compares the bytes Rust produces with the file the contract names, rather than comparing two
/// copies of a name.
///
/// Two properties make this more than a spot check. The validator fails on any JSON Schema keyword it does not
/// implement, so the contract cannot quietly start using a keyword that would go unchecked. And
/// `every_registered_handler_is_covered` reads this file's own `generate_handler![...]` list, so registering a
/// fifth handler without a shape test for it is a test failure rather than an omission nobody notices.
#[cfg(test)]
mod wire_shape_tests {
    use super::*;
    use serde::Serialize;
    use serde_json::Value;

    const PAYLOAD_TYPES: &str =
        include_str!("../../../../schemas/tauri-bridge-v1/payload-types.json");

    /// The operations this module checks the shape of. Kept as data so `every_registered_handler_is_covered`
    /// can compare it against the registration list rather than against a comment.
    const COVERED_OPERATIONS: &[&str] = &[
        "create_project",
        "list_projects",
        "get_recovery_status",
        "validate_workspace",
        "replay_dead_letter",
        "get_communication_health",
        "get_event_cursor",
    ];

    /// The JSON Schema keywords this validator implements. A validated type that uses anything else is a test
    /// failure, not a silently ignored constraint: under-validation that reports success is worse than no
    /// check, because it is indistinguishable from a check that passed.
    const SUPPORTED_KEYWORDS: &[&str] = &[
        "$ref",
        "title",
        "description",
        "type",
        "required",
        "properties",
        "additionalProperties",
        "enum",
        "items",
        "minimum",
        "minLength",
    ];

    fn contract() -> Value {
        serde_json::from_str(PAYLOAD_TYPES).expect("payload-types.json is not parseable JSON")
    }

    /// Follow `#/...` references until a concrete schema is reached. A reference that does not resolve is a
    /// failure rather than a skip, because a declaration that resolves to nothing validates nothing.
    fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
        let mut current = schema;
        for _ in 0..8 {
            let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
                return current;
            };
            let path = reference
                .strip_prefix("#/")
                .unwrap_or_else(|| panic!("unsupported $ref {reference}"));
            let mut node = root;
            for segment in path.split('/') {
                node = node.get(segment).unwrap_or_else(|| {
                    panic!("$ref {reference} does not resolve in payload-types.json")
                });
            }
            current = node;
        }
        panic!("$ref chain is deeper than 8 hops; payload-types.json is probably circular");
    }

    fn type_matches(want: &str, value: &Value) -> bool {
        match want {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            _ => false,
        }
    }

    fn validate(root: &Value, schema: &Value, value: &Value, at: &str, problems: &mut Vec<String>) {
        let schema = resolve(root, schema);
        if let Some(object) = schema.as_object() {
            for keyword in object.keys() {
                if !SUPPORTED_KEYWORDS.contains(&keyword.as_str()) {
                    problems.push(format!(
                        "{at}: the contract uses the JSON Schema keyword \"{keyword}\", which this validator does \
                         not implement. Add it here rather than leaving the shape unchecked."
                    ));
                }
            }
        }
        if let Some(declared) = schema.get("type") {
            let wanted: Vec<&str> = match declared {
                Value::String(one) => vec![one.as_str()],
                Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            if !wanted.iter().any(|want| type_matches(want, value)) {
                problems.push(format!(
                    "{at} must be {}, found {}",
                    wanted.join("|"),
                    json_kind(value)
                ));
                return;
            }
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                problems.push(format!("{at} must be one of {allowed:?}, found {value}"));
            }
        }
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64) {
            if let Some(number) = value.as_i64() {
                if number < minimum {
                    problems.push(format!("{at} must be >= {minimum}, found {number}"));
                }
            }
        }
        if let Some(min_length) = schema.get("minLength").and_then(Value::as_u64) {
            if let Some(text) = value.as_str() {
                if (text.chars().count() as u64) < min_length {
                    problems.push(format!("{at} must be at least {min_length} character(s)"));
                }
            }
        }
        if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
            for (index, item) in array.iter().enumerate() {
                validate(root, items, item, &format!("{at}[{index}]"), problems);
            }
        }
        if let Some(object) = value.as_object() {
            let properties = schema.get("properties").and_then(Value::as_object);
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for key in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(key) {
                        problems.push(format!("{at} is missing required key \"{key}\""));
                    }
                }
            }
            if let Some(properties) = properties {
                for (key, sub) in properties {
                    if let Some(field) = object.get(key) {
                        validate(root, sub, field, &format!("{at}.{key}"), problems);
                    }
                }
            }
            if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                if let Some(properties) = properties {
                    for key in object.keys() {
                        if !properties.contains_key(key) {
                            problems.push(format!(
                                "{at} declares \"{key}\", which the declared type does not permit"
                            ));
                        }
                    }
                }
            }
        }
    }

    fn json_kind(value: &Value) -> &'static str {
        match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    /// Serialize a real wire value and validate it against the named declared type.
    fn conforms<T: Serialize>(type_name: &str, value: &T) {
        let root = contract();
        let schema = root
            .get("types")
            .and_then(|types| types.get(type_name))
            .unwrap_or_else(|| {
                panic!(
                    "payload-types.json declares no type \"{type_name}\", so the shape of the value that claims \
                     to be one is unconstrained"
                )
            });
        let serialized =
            serde_json::to_value(value).expect("a wire struct failed to serialize as JSON");
        let mut problems = Vec::new();
        validate(&root, schema, &serialized, type_name, &mut problems);
        assert!(
            problems.is_empty(),
            "{type_name} does not conform to its declared payload type:\n  - {}\nserialized: {serialized}",
            problems.join("\n  - ")
        );
    }

    fn project_view() -> ProjectView {
        ProjectView {
            project_id: "prj_2f1c9a4b6e0d3857".to_string(),
            name: "proj".to_string(),
            local_path: "C:\\work\\proj".to_string(),
            phase: "DISCOVERY".to_string(),
            status: "ACTIVE".to_string(),
            current_epoch: 0,
            brief_id: Some("brf_7d3e5c1a9b204f68".to_string()),
            brief_version: Some(1),
            brief_body: Some("Build something real.".to_string()),
            created_at: "2026-10-05T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn create_project_response_conforms() {
        conforms("create_projectResponse", &project_view());
    }

    #[test]
    fn list_projects_response_conforms() {
        conforms(
            "list_projectsResponse",
            &vec![project_view(), project_view()],
        );
    }

    #[test]
    fn a_projection_with_no_brief_conforms() {
        // The nullable fields are sent as null rather than omitted, so the null case is part of the shape and
        // is checked rather than assumed.
        let no_brief = ProjectView {
            brief_id: None,
            brief_version: None,
            brief_body: None,
            ..project_view()
        };
        conforms("create_projectResponse", &no_brief);
    }

    #[test]
    fn get_recovery_status_response_conforms() {
        conforms(
            "get_recovery_statusResponse",
            &RecoveryView {
                clean: true,
                integrity_ok: true,
                issues: Vec::new(),
            },
        );
        conforms(
            "get_recovery_statusResponse",
            &RecoveryView {
                clean: false,
                integrity_ok: false,
                issues: vec![RecoveryIssueView {
                    kind: "ORPHANED_BRIEF".to_string(),
                    detail: "brief has no project".to_string(),
                }],
            },
        );
    }

    #[test]
    fn validate_workspace_response_conforms() {
        conforms(
            "validate_workspaceResponse",
            &WorkspaceCheck {
                status: "AUTHORIZED",
                canonical_path: Some("C:\\work\\proj".to_string()),
                requested_path: "C:\\work\\proj".to_string(),
                derived_project_name: Some("proj".to_string()),
                code: None,
                message: None,
            },
        );
        conforms(
            "validate_workspaceResponse",
            &WorkspaceCheck {
                status: "INVALID",
                canonical_path: None,
                requested_path: "Z:\\nope".to_string(),
                derived_project_name: None,
                code: Some("WORKSPACE_DOES_NOT_EXIST".to_string()),
                message: Some("That folder does not exist.".to_string()),
            },
        );
    }

    #[test]
    fn a_rejection_conforms_to_the_error_payload_type() {
        // The error payload is declared once, under `error`, because a rejection is not operation-specific.
        let root = contract();
        let schema = root
            .get("error")
            .expect("payload-types.json declares no error payload type");
        let serialized = serde_json::to_value(CommandError {
            code: "WORKSPACE_DOES_NOT_EXIST",
            message: "That folder does not exist.".to_string(),
        })
        .expect("CommandError failed to serialize");
        let mut problems = Vec::new();
        validate(&root, schema, &serialized, "error", &mut problems);
        assert!(
            problems.is_empty(),
            "error does not conform:\n  - {}",
            problems.join("\n  - ")
        );
    }

    #[test]
    fn get_communication_health_response_conforms() {
        conforms(
            "get_communication_healthResponse",
            &CommunicationHealthView {
                project_id: "prj_a".to_string(),
                backlog: 0,
                enqueue_limit: 1024,
                counts: DeliveryCountsView {
                    queued: 0,
                    retrying: 0,
                    expired: 0,
                    dead_lettered: 0,
                },
                open_gaps: vec![OpenGapView {
                    code: "SEQUENCE_GAP".to_string(),
                    session_id: "ses_a".to_string(),
                    channel: "TASK".to_string(),
                    expected: 2,
                    found: 3,
                }],
                transport_status: "not_connected".to_string(),
            },
        );
    }

    #[test]
    fn get_communication_health_with_no_gaps_conforms() {
        conforms(
            "get_communication_healthResponse",
            &CommunicationHealthView {
                project_id: "prj_a".to_string(),
                backlog: 7,
                enqueue_limit: 1024,
                counts: DeliveryCountsView {
                    queued: 7,
                    retrying: 1,
                    expired: 2,
                    dead_lettered: 3,
                },
                open_gaps: Vec::new(),
                transport_status: "not_connected".to_string(),
            },
        );
    }

    #[test]
    fn get_event_cursor_response_conforms() {
        // The absent-optional case, which is what a fresh consumer actually receives.
        conforms(
            "get_event_cursorResponse",
            &EventCursorView {
                project_id: "prj_a".to_string(),
                consumer_id: "ui".to_string(),
                last_sequence: 0,
                next_sequence: None,
                gap_detected: false,
                resync_from: None,
            },
        );
    }

    #[test]
    fn get_event_cursor_response_with_a_position_conforms() {
        conforms(
            "get_event_cursorResponse",
            &EventCursorView {
                project_id: "prj_a".to_string(),
                consumer_id: "ui".to_string(),
                last_sequence: 42,
                next_sequence: Some(43),
                gap_detected: true,
                resync_from: Some(44),
            },
        );
    }
    #[test]
    fn replay_dead_letter_response_conforms() {
        conforms(
            "replay_dead_letterResponse",
            &ReplayDeadLetterView {
                replayed_message_id: "msg_replayed".to_string(),
                source_message_id: "msg_dead".to_string(),
                context_snapshot_id: None,
                state_digest: None,
                context_refreshed: false,
                deduplicated: false,
            },
        );
    }

    #[test]
    fn replay_dead_letter_response_with_context_conforms() {
        conforms(
            "replay_dead_letterResponse",
            &ReplayDeadLetterView {
                replayed_message_id: "msg_replayed".to_string(),
                source_message_id: "msg_dead".to_string(),
                context_snapshot_id: Some("ctx_original".to_string()),
                state_digest: Some(
                    "a3f1c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956".to_string(),
                ),
                context_refreshed: false,
                deduplicated: true,
            },
        );
    }

    /// A handler registered in `generate_handler![...]` with no shape test is a wire surface nothing checks.
    #[test]
    fn every_registered_handler_is_covered() {
        let source = include_str!("main.rs");
        // Comment lines are removed first. The macro name is mentioned in prose in this file, and a comment that
        // happened to sit above the real registration would otherwise be parsed as the registration list, which
        // would make this test pass for the wrong reason or fail for no reason.
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let list = code
            .split_once("generate_handler![")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| list)
            .expect("could not find the generate_handler![...] list in this file");
        let registered: Vec<&str> = list
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.rsplit("::").next().unwrap_or(name))
            .collect();
        for name in &registered {
            assert!(
                COVERED_OPERATIONS.contains(name),
                "{name} is registered in generate_handler![...] but no shape test covers it. Add a conformance \
                 test for the payload it returns, and add it to COVERED_OPERATIONS."
            );
        }
        for name in COVERED_OPERATIONS {
            assert!(
                registered.contains(name),
                "COVERED_OPERATIONS names {name}, which generate_handler![...] does not register. The list of \
                 covered operations must describe the handlers that exist."
            );
        }
    }
}
