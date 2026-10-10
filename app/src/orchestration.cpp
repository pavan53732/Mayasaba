// Orchestrator: controller application services. Owns cross-service scheduling, barriers,
// registered decision triggers, and phase transitions. It does NOT mutate another service's
// authoritative records (project epoch, requirements, decisions, messages) — it coordinates
// and dispatches through each engine's registered commands. All agent process launch goes
// through the gateway/kernel stack; this file never spawns a process directly.
//
// Persistence ownership: the projects/contributions tables are the project service's records
// (this vertical slice hosts that service); triggers are orchestrator-owned scheduling state.
// Task/lease/attempt records live in the Task engine; council records in the Council engine.
#include "mayasaba/orchestrator.hpp"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "mayasaba/adapter.hpp"
#include "mayasaba/base.hpp"
#include "mayasaba/council.hpp"
#include "mayasaba/evidence.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/gateway.hpp"
#include "mayasaba/policy.hpp"
#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/tasks.hpp"
#include "mayasaba/validation.hpp"
#include "mayasaba/workspace.hpp"

using mayasaba::storage::Row;
using mayasaba::storage::SqlValue;

namespace mayasaba::app {

const char* TriggerKindName(TriggerKind kind) {
    switch (kind) {
        case TriggerKind::ExplorationComplete: return "ExplorationComplete";
        case TriggerKind::CouncilPointResolved: return "CouncilPointResolved";
        case TriggerKind::LeaseExpired: return "LeaseExpired";
        case TriggerKind::ValidationPassed: return "ValidationPassed";
        case TriggerKind::UserMilestone: return "UserMilestone";
    }
    return "Unknown";
}

const char* ProjectPhaseName(ProjectPhase phase) {
    switch (phase) {
        case ProjectPhase::Unbound: return "UNBOUND";
        case ProjectPhase::Bound: return "BOUND";
        case ProjectPhase::Exploration: return "EXPLORATION";
        case ProjectPhase::Execution: return "EXECUTION";
        case ProjectPhase::Integrating: return "INTEGRATING";
        case ProjectPhase::Validation: return "VALIDATION";
        case ProjectPhase::Completed: return "COMPLETED";
        case ProjectPhase::Paused: return "PAUSED";
        case ProjectPhase::Closed: return "CLOSED";
        case ProjectPhase::Abandoned: return "ABANDONED";
    }
    return "UNKNOWN";
}

std::optional<ProjectPhase> ParseProjectPhase(const std::string& name) {
    std::string upper;
    upper.reserve(name.size());
    for (char c : name) {
        upper.push_back(static_cast<char>(std::toupper(static_cast<unsigned char>(c))));
    }
    if (upper == "UNBOUND") return ProjectPhase::Unbound;
    if (upper == "BOUND") return ProjectPhase::Bound;
    if (upper == "EXPLORATION") return ProjectPhase::Exploration;
    if (upper == "EXECUTION") return ProjectPhase::Execution;
    if (upper == "INTEGRATING") return ProjectPhase::Integrating;
    if (upper == "VALIDATION") return ProjectPhase::Validation;
    if (upper == "COMPLETED") return ProjectPhase::Completed;
    if (upper == "PAUSED") return ProjectPhase::Paused;
    if (upper == "CLOSED") return ProjectPhase::Closed;
    if (upper == "ABANDONED") return ProjectPhase::Abandoned;
    return std::nullopt;
}

namespace {

Status EnsureTriggerSchema(storage::Store* store) {
    return store->Exec(
        "CREATE TABLE IF NOT EXISTS orchestrator_triggers("
        "trigger_id TEXT PRIMARY KEY, kind TEXT NOT NULL, project_id TEXT NOT NULL,"
        "target_task_id TEXT NOT NULL DEFAULT '', council_point_id TEXT NOT NULL DEFAULT '',"
        "fired INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL,"
        "fired_at TEXT NOT NULL DEFAULT '')");
}

}  // namespace

Status Orchestrator::ValidatePhaseTransition(ProjectPhase from, ProjectPhase to) {
    switch (from) {
        case ProjectPhase::Unbound:
            return to == ProjectPhase::Bound || to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "invalid phase transition");
        case ProjectPhase::Bound:
            return to == ProjectPhase::Exploration || to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "invalid phase transition");
        case ProjectPhase::Exploration:
            return to == ProjectPhase::Execution || to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "invalid phase transition");
        case ProjectPhase::Execution:
            return to == ProjectPhase::Integrating || to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "invalid phase transition");
        case ProjectPhase::Integrating:
            return to == ProjectPhase::Validation || to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "invalid phase transition");
        case ProjectPhase::Validation:
            return to == ProjectPhase::Completed || to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "invalid phase transition");
        case ProjectPhase::Completed:
        case ProjectPhase::Paused:
        case ProjectPhase::Closed:
        case ProjectPhase::Abandoned:
            // Terminals: only explicit close/abandon transitions are valid.
            return to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "terminal phase cannot transition");
    }
    return Status::Error(ErrorCode::Conflict, "invalid source phase");
}

Expected<ProjectIdentity> Orchestrator::LoadProject(const std::string& project_id) {
    if (project_id.empty()) {
        return Fail<ProjectIdentity>(ErrorCode::InvalidArgument, "project_id is empty");
    }
    auto rows = services_.store->Query(
        "SELECT project_id, root_canonical, root_path, epoch, created_at FROM projects "
        "WHERE project_id = ?1",
        {SqlValue::Text(project_id)});
    if (!rows.ok()) return Fail<ProjectIdentity>(rows.code(), rows.message());
    if (rows->empty()) {
        return Fail<ProjectIdentity>(ErrorCode::NotFound, "project not found: " + project_id);
    }
    const Row& row = rows->front();
    ProjectIdentity identity;
    identity.project_id = row.Text("project_id");
    identity.canonical_root = row.Text("root_canonical");
    identity.display_path = row.Text("root_path");
    identity.epoch = row.Int("epoch");
    identity.created_at = row.Text("created_at");
    return identity;
}

Expected<ProjectIdentity> Orchestrator::BindProjectRoot(const std::string& display_path,
                                                        const std::string& canonical_root) {
    std::lock_guard<std::mutex> lock(mutex_);
    const std::string candidate = canonical_root.empty() ? display_path : canonical_root;
    auto validation = workspace::ValidateRoot(candidate);
    if (!validation.ok) {
        return Fail<ProjectIdentity>(ErrorCode::InvalidArgument,
                                     "project root rejected: " + validation.reason);
    }

    // Restore an existing binding: opening a folder restores project identity and never
    // launches agents, scans the repository, initializes Git, or consumes provider usage.
    auto existing = services_.store->Query(
        "SELECT project_id, root_canonical, root_path, epoch, created_at FROM projects "
        "WHERE root_canonical = ?1",
        {SqlValue::Text(validation.canonical_path)});
    if (!existing.ok()) return Fail<ProjectIdentity>(existing.code(), existing.message());
    if (!existing->empty()) {
        const Row& row = existing->front();
        ProjectIdentity identity;
        identity.project_id = row.Text("project_id");
        identity.canonical_root = row.Text("root_canonical");
        identity.display_path = row.Text("root_path");
        identity.epoch = row.Int("epoch");
        identity.created_at = row.Text("created_at");
        return identity;
    }

    ProjectIdentity identity;
    identity.project_id = NewId("proj");
    identity.canonical_root = validation.canonical_path;
    identity.display_path = display_path.empty() ? validation.display_path : display_path;
    identity.epoch = 1;
    identity.created_at = NowUtcIso8601();

    std::string name = identity.display_path;
    if (const auto slash = name.find_last_of("\\/");
        slash != std::string::npos && slash + 1 < name.size()) {
        name = name.substr(slash + 1);
    }

    auto inserted = services_.store->Exec(
        "INSERT INTO projects(project_id, name, root_path, root_canonical, volume_serial,"
        " file_index, epoch, lifecycle_phase, condition, created_at, updated_at)"
        " VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ACTIVE', ?9, ?9)",
        {SqlValue::Text(identity.project_id), SqlValue::Text(name),
         SqlValue::Text(identity.display_path), SqlValue::Text(identity.canonical_root),
         SqlValue::Int(static_cast<std::int64_t>(validation.volume_serial)),
         SqlValue::Int(static_cast<std::int64_t>(validation.file_index)),
         SqlValue::Int(identity.epoch), SqlValue::Text(ProjectPhaseName(ProjectPhase::Bound)),
         SqlValue::Text(identity.created_at)});
    if (!inserted.ok()) return Fail<ProjectIdentity>(inserted.code(), inserted.message());

    auto event = services_.store->AppendEvent(
        identity.project_id, "project.bound",
        {{"canonical_root", identity.canonical_root},
         {"display_path", identity.display_path},
         {"epoch", identity.epoch}});
    if (!event.ok()) return Fail<ProjectIdentity>(event.code(), event.message());
    return identity;
}

Expected<ProjectIdentity> Orchestrator::Project(const std::string& project_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    return LoadProject(project_id);
}

Expected<SendResult> Orchestrator::SubmitUserContribution(
    const std::string& project_id, const std::string& text,
    const std::vector<std::string>& attachments) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto project = LoadProject(project_id);
    if (!project.ok()) return Fail<SendResult>(project.code(), project.message());

    ChatContribution contribution;
    contribution.contribution_id = NewId("con");
    contribution.project_id = project_id;
    contribution.text = text;
    contribution.attachments = attachments;
    contribution.created_at = NowUtcIso8601();

    nlohmann::json attachment_json = nlohmann::json::array();
    for (const auto& attachment : attachments) attachment_json.push_back(attachment);

    auto inserted = services_.store->Exec(
        "INSERT INTO contributions(contribution_id, project_id, text, attachments, state,"
        " created_at) VALUES(?1, ?2, ?3, ?4, 'PERSISTED', ?5)",
        {SqlValue::Text(contribution.contribution_id), SqlValue::Text(project_id),
         SqlValue::Text(text), SqlValue::Text(attachment_json.dump()),
         SqlValue::Text(contribution.created_at)});
    if (!inserted.ok()) return Fail<SendResult>(inserted.code(), inserted.message());

    auto event = services_.store->AppendEvent(
        project_id, "contribution.persisted",
        {{"contribution_id", contribution.contribution_id},
         {"attachment_count", attachments.size()}});
    if (!event.ok()) return Fail<SendResult>(event.code(), event.message());

    SendResult result;
    result.contribution_id = contribution.contribution_id;
    result.outcome = "PERSISTED";
    result.detail = "contribution persisted; persistence alone starts no agent work";
    return result;
}

Expected<ChatContribution> Orchestrator::Contribution(const std::string& contribution_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto rows = services_.store->Query(
        "SELECT contribution_id, project_id, text, attachments, created_at FROM contributions "
        "WHERE contribution_id = ?1",
        {SqlValue::Text(contribution_id)});
    if (!rows.ok()) return Fail<ChatContribution>(rows.code(), rows.message());
    if (rows->empty()) {
        return Fail<ChatContribution>(ErrorCode::NotFound,
                                      "contribution not found: " + contribution_id);
    }
    const Row& row = rows->front();
    ChatContribution contribution;
    contribution.contribution_id = row.Text("contribution_id");
    contribution.project_id = row.Text("project_id");
    contribution.text = row.Text("text");
    contribution.created_at = row.Text("created_at");
    auto parsed = nlohmann::json::parse(row.Text("attachments"), nullptr, false);
    if (!parsed.is_discarded() && parsed.is_array()) {
        for (const auto& entry : parsed) {
            if (entry.is_string()) contribution.attachments.push_back(entry.get<std::string>());
        }
    }
    return contribution;
}

Expected<tasks::Task> Orchestrator::ScheduleTask(const std::string& project_id,
                                                 const std::string& title,
                                                 const tasks::TaskContract& contract) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto project = LoadProject(project_id);
    if (!project.ok()) return Fail<tasks::Task>(project.code(), project.message());

    tasks::TaskContract validated = contract;
    if (validated.project_id.empty()) validated.project_id = project_id;
    if (validated.task_id.empty()) validated.task_id = NewId("task");

    auto task = services_.tasks->RegisterTask(title, validated);
    if (!task.ok()) return task;

    auto event = services_.store->AppendEvent(
        project_id, "task.registered",
        {{"task_id", task->task_id}, {"title", title},
         {"objective", validated.objective},
         {"criteria_count", validated.criteria.size()}});
    if (!event.ok()) return Fail<tasks::Task>(event.code(), event.message());
    return task;
}

Expected<tasks::Lease> Orchestrator::LeaseTask(const std::string& task_id,
                                               const std::string& agent) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto task = services_.tasks->GetTask(task_id);
    if (!task.ok()) return Fail<tasks::Lease>(task.code(), task.message());
    if (!adapters::ParseAgentId(agent).has_value()) {
        return Fail<tasks::Lease>(ErrorCode::InvalidArgument,
                                  "unknown agent id (expected hermes|kilo|claude): " + agent);
    }

    // A lease may only be granted to a READY task. Evaluate the schedule first: it promotes
    // PENDING tasks whose dependencies and write sets are satisfied, and reports the exact
    // reason for anything still blocked.
    if (task->state == tasks::TaskState::Pending) {
        auto decision = services_.tasks->EvaluateSchedule(task->project_id);
        if (!decision.ok()) return Fail<tasks::Lease>(decision.code(), decision.message());
        task = services_.tasks->GetTask(task_id);
        if (!task.ok()) return Fail<tasks::Lease>(task.code(), task.message());
    }
    if (task->state != tasks::TaskState::Ready && task->state != tasks::TaskState::Leased) {
        return Fail<tasks::Lease>(
            ErrorCode::NotReady,
            "task " + task_id + " is not leasable (state=" +
                tasks::TaskStateName(task->state) + ")");
    }

    const std::string attempt_id = NewId("att");
    auto lease = services_.tasks->GrantLease(task_id, attempt_id, agent);
    if (!lease.ok()) return lease;

    auto project = LoadProject(task->project_id);
    if (!project.ok()) return Fail<tasks::Lease>(project.code(), project.message());

    // Register the effective scope grants for the leased task: read the project root, write
    // only in isolated staging (added at execution start), publish through the journal.
    policy::GrantSet grants;
    grants.readable_roots.push_back(project->canonical_root);
    grants.publish_roots.push_back(project->canonical_root);
    grants.allow_process_launch = true;
    grants.allow_agent_session = true;
    grants.allow_native_web_tools = std::find(task->contract.approved_capabilities.begin(),
                                              task->contract.approved_capabilities.end(),
                                              "native-web-search") !=
                                    task->contract.approved_capabilities.end();
    grants.authority_ref = "task:" + task_id;
    auto granted = services_.policy->Grant(task_id, grants);
    if (!granted.ok()) return Fail<tasks::Lease>(granted.code(), granted.message());

    auto event = services_.store->AppendEvent(
        task->project_id, "task.leased",
        {{"task_id", task_id}, {"agent", agent}, {"lease_id", lease->lease_id},
         {"lease_version", lease->version}, {"attempt_id", attempt_id}});
    if (!event.ok()) return Fail<tasks::Lease>(event.code(), event.message());
    return lease;
}

std::string Orchestrator::WorkspaceRoot() const {
    const std::string& db_path = services_.store->path();
    const auto slash = db_path.find_last_of("\\/");
    const std::string dir =
        (slash == std::string::npos) ? std::string(".") : db_path.substr(0, slash);
    return dir + "\\workspace";
}

Expected<workspace::StagedWorkspace> Orchestrator::EnsureStaging(
    const tasks::Task& task, const ProjectIdentity& project) {
    if (const auto it = staging_.find(task.task_id); it != staging_.end()) {
        return it->second;
    }

    // Authorized read scope: the contract's explicit paths, or the whole root when the
    // contract declares no narrower scope. Heavy/generated directories are never staged.
    std::vector<std::string> scope = task.contract.allowed_read_paths;
    if (scope.empty()) {
        static const std::vector<std::string> kExcludedTopLevel = {
            ".git", ".kilo", ".staging", ".workbuddy-ai", ".mayasaba", "node_modules",
            ".venv", "venv", "target", "dist", "build", "__pycache__"};
        std::error_code ec;
        std::filesystem::directory_iterator it(project.canonical_root, ec);
        if (ec) {
            return Fail<workspace::StagedWorkspace>(
                ErrorCode::IoError, "cannot enumerate project root: " + ec.message());
        }
        for (const auto& entry : it) {
            const std::string name = entry.path().filename().string();
            if (std::find(kExcludedTopLevel.begin(), kExcludedTopLevel.end(), name) !=
                kExcludedTopLevel.end()) {
                continue;
            }
            scope.push_back(name);
        }
    }

    auto staged = workspace::CreateStaging(project.project_id, task.task_id,
                                           project.canonical_root, scope, WorkspaceRoot());
    if (!staged.ok()) return staged;
    staging_[task.task_id] = *staged;
    return *staged;
}

Status Orchestrator::GrantTaskScopes(const tasks::Task& task, const ProjectIdentity& project,
                                     const std::string& writable_root) {
    policy::GrantSet grants;
    grants.readable_roots.push_back(project.canonical_root);
    if (!writable_root.empty()) grants.writable_roots.push_back(writable_root);
    grants.publish_roots.push_back(project.canonical_root);
    grants.allow_process_launch = true;
    grants.allow_agent_session = true;
    grants.allow_native_web_tools = std::find(task.contract.approved_capabilities.begin(),
                                              task.contract.approved_capabilities.end(),
                                              "native-web-search") !=
                                    task.contract.approved_capabilities.end();
    grants.authority_ref = "task:" + task.task_id;
    return services_.policy->Grant(task.task_id, grants);
}

std::string Orchestrator::BuildTaskPrompt(const tasks::Task& task) const {
    std::string prompt;
    prompt += "Task: " + task.title + "\n\nObjective:\n" + task.contract.objective + "\n";
    if (!task.contract.criteria.empty()) {
        prompt += "\nAcceptance criteria:\n";
        for (const auto& criterion : task.contract.criteria) {
            prompt += "- [" + criterion.criterion_id + "] " + criterion.expectation +
                      " (oracle: " + criterion.oracle_class + ")\n";
        }
    }
    prompt +=
        "\nWork only inside the current working directory (an isolated staging copy). Do not "
        "modify files outside it. State what you changed and how it can be verified. Do not "
        "request credentials, install plugins, or change model/provider configuration.\n";
    return prompt;
}

Expected<std::string> Orchestrator::StartTaskExecution(const std::string& task_id,
                                                       const std::string& attempt_id,
                                                       const std::string& agent) {
    std::lock_guard<std::mutex> lock(mutex_);
    (void)attempt_id;   // reserved for idempotent retries; the engine's attempt id is authoritative

    auto task = services_.tasks->GetTask(task_id);
    if (!task.ok()) return Fail<std::string>(task.code(), task.message());
    auto project = LoadProject(task->project_id);
    if (!project.ok()) return Fail<std::string>(project.code(), project.message());

    auto kind = adapters::ParseAgentId(agent);
    if (!kind.has_value()) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "unknown agent id: " + agent);
    }

    auto staged = EnsureStaging(*task, *project);
    if (!staged.ok()) return Fail<std::string>(staged.code(), staged.message());
    auto scopes = GrantTaskScopes(*task, *project, staged->path);
    if (!scopes.ok()) return Fail<std::string>(scopes.code(), scopes.message());

    auto lease = services_.tasks->ActiveLeaseFor(task_id);
    if (!lease.ok()) {
        return Fail<std::string>(ErrorCode::NotReady,
                                 "no active lease for task " + task_id +
                                     "; lease the task before starting execution");
    }

    nlohmann::json baseline = {{"staging_path", staged->path},
                               {"baseline_digest", staged->baseline_digest},
                               {"root", project->canonical_root}};
    auto attempt = services_.tasks->StartAttempt(task_id, agent, lease->version, baseline);
    if (!attempt.ok()) return Fail<std::string>(attempt.code(), attempt.message());

    adapters::SessionSpec spec;
    spec.project_id = task->project_id;
    spec.project_root = project->canonical_root;
    spec.execution_working_directory = staged->path;
    spec.workspace_view_id = staged->workspace_view_id;
    spec.task_id = task_id;
    spec.attempt_id = attempt->attempt_id;
    spec.lease_id = lease->lease_id;
    spec.lease_version = lease->version;
    spec.project_epoch = project->epoch;
    spec.allowed_relative_paths = task->contract.allowed_read_paths;
    spec.allowed_write_paths = task->contract.allowed_write_paths;
    spec.read_only = false;
    spec.allow_native_web_tools = std::find(task->contract.approved_capabilities.begin(),
                                            task->contract.approved_capabilities.end(),
                                            "native-web-search") !=
                                  task->contract.approved_capabilities.end();
    spec.prompt = BuildTaskPrompt(*task);

    const std::string session_id = NewId("ses");
    auto started = services_.gateway->StartSession(*kind, spec, session_id);
    if (!started.ok()) {
        services_.tasks->FinishAttempt(attempt->attempt_id, "FAILED",
                                       {{"error", started.message()}});
        return Fail<std::string>(started.code(), started.message());
    }

    task_sessions_[session_id] =
        TaskSessionLink{task_id, attempt->attempt_id, lease->version, staged->path};

    auto event = services_.store->AppendEvent(
        task->project_id, "task.execution.started",
        {{"task_id", task_id}, {"attempt_id", attempt->attempt_id}, {"agent", agent},
         {"session_id", session_id}, {"lease_version", lease->version}});
    if (!event.ok()) return Fail<std::string>(event.code(), event.message());
    return session_id;
}

Status Orchestrator::OnAgentSessionCompleted(const std::string& session_id, bool success,
                                             const std::string& session_error) {
    std::lock_guard<std::mutex> lock(mutex_);
    const auto it = task_sessions_.find(session_id);
    if (it == task_sessions_.end()) {
        return Status::Ok();   // not a task session; council sessions are routed by the caller
    }
    const TaskSessionLink link = it->second;
    task_sessions_.erase(it);

    nlohmann::json result = {{"session_id", session_id}, {"session_error", session_error}};
    auto finished = services_.tasks->FinishAttempt(
        link.attempt_id, success ? "SUCCEEDED" : "FAILED", result);

    auto task = services_.tasks->GetTask(link.task_id);
    if (!task.ok()) return finished.ok() ? task.status() : finished;

    // Evidence: the session outcome is recorded as a verified controller observation. Agent
    // narrative alone is never acceptance evidence; this record proves what happened, not
    // that the work is correct.
    evidence::EvidenceRecord draft;
    draft.project_id = task->project_id;
    draft.check_id = "agent-session-outcome";
    draft.check_version = "1";
    draft.artifact.path = link.staging_path;
    draft.artifact.kind = "DIRECTORY";
    draft.artifact.provenance = "agent:session:" + session_id;
    draft.artifact.collected_at = NowUtcIso8601();
    draft.outcome = success ? "PASS" : "FAIL";
    draft.detail = session_error;
    draft.collector = "controller";
    draft.collected_at = NowUtcIso8601();
    services_.evidence->Collect(draft);   // integrity-checked at collection time

    const auto next = success ? tasks::TaskState::Integrating : tasks::TaskState::Failed;
    auto transitioned = services_.tasks->TransitionTask(link.task_id, next);
    auto event = services_.store->AppendEvent(
        task->project_id, "task.execution.completed",
        {{"task_id", link.task_id}, {"attempt_id", link.attempt_id},
         {"session_id", session_id}, {"success", success}, {"session_error", session_error},
         {"task_state", tasks::TaskStateName(next)}});
    if (!finished.ok()) return finished;
    if (!transitioned.ok()) return transitioned;
    if (!event.ok()) return event.status();
    return Status::Ok();
}

Expected<workspace::IntegrationCandidate> Orchestrator::PrepareIntegration(
    const std::string& task_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto task = services_.tasks->GetTask(task_id);
    if (!task.ok()) {
        return Fail<workspace::IntegrationCandidate>(task.code(), task.message());
    }
    auto project = LoadProject(task->project_id);
    if (!project.ok()) {
        return Fail<workspace::IntegrationCandidate>(project.code(), project.message());
    }
    auto staged = EnsureStaging(*task, *project);
    if (!staged.ok()) {
        return Fail<workspace::IntegrationCandidate>(staged.code(), staged.message());
    }
    return workspace::BuildIntegrationCandidate(*staged, project->canonical_root);
}

Expected<workspace::PublicationOutcome> Orchestrator::PublishIntegration(
    const std::string& task_id, const std::string& recovery_root) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto task = services_.tasks->GetTask(task_id);
    if (!task.ok()) return Fail<workspace::PublicationOutcome>(task.code(), task.message());
    auto project = LoadProject(task->project_id);
    if (!project.ok()) {
        return Fail<workspace::PublicationOutcome>(project.code(), project.message());
    }
    auto staged = EnsureStaging(*task, *project);
    if (!staged.ok()) {
        return Fail<workspace::PublicationOutcome>(staged.code(), staged.message());
    }

    auto candidate = workspace::BuildIntegrationCandidate(*staged, project->canonical_root);
    if (!candidate.ok()) {
        return Fail<workspace::PublicationOutcome>(candidate.code(), candidate.message());
    }
    if (!candidate->conflicts.empty()) {
        // Fail closed: the user's root changed since the baseline. Never overwrite newer edits.
        workspace::PublicationOutcome outcome;
        outcome.publication_id = NewId("pub");
        outcome.state = "CONFLICT";
        outcome.conflicted_paths = candidate->conflicts;
        outcome.detail = "root diverged from the staged baseline; publication refused";
        services_.store->AppendEvent(task->project_id, "publication.conflict",
                                     {{"task_id", task_id}, {"conflicts", candidate->conflicts}});
        return outcome;
    }

    workspace::PublicationPlan plan;
    plan.publication_id = NewId("pub");
    plan.project_id = task->project_id;
    plan.task_id = task_id;
    plan.changes = candidate->changes;
    plan.created_at = NowUtcIso8601();

    const std::string recovery =
        recovery_root.empty() ? (WorkspaceRoot() + "\\recovery") : recovery_root;
    auto ensure_recovery = fs::EnsureDirectory(recovery);
    if (!ensure_recovery.ok()) {
        return Fail<workspace::PublicationOutcome>(ensure_recovery.code(),
                                                   ensure_recovery.message());
    }
    // The publication journal locates the controller-owned staged content under the recovery
    // root (staged file = recovery_root/<rel_path>), so materialize each staged change there
    // before publishing. Deletions carry no staged content.
    for (const auto& change : candidate->changes) {
        if (change.operation == "DELETE") continue;
        auto bytes = fs::ReadFileBytes(fs::JoinPath(staged->path, change.rel_path));
        if (!bytes.ok()) {
            return Fail<workspace::PublicationOutcome>(
                bytes.code(), "cannot read staged file " + change.rel_path + ": " +
                                  bytes.message());
        }
        const std::string destination = fs::JoinPath(recovery, change.rel_path);
        if (const auto slash = destination.find_last_of("\\/"); slash != std::string::npos) {
            auto parent = fs::EnsureDirectory(destination.substr(0, slash));
            if (!parent.ok()) {
                return Fail<workspace::PublicationOutcome>(parent.code(), parent.message());
            }
        }
        auto written = fs::WriteFileBytes(destination, *bytes);
        if (!written.ok()) {
            return Fail<workspace::PublicationOutcome>(
                written.code(), "cannot stage recovery material for " + change.rel_path + ": " +
                                    written.message());
        }
    }

    auto outcome = workspace::Publish(plan, project->canonical_root, services_.store, recovery);
    if (!outcome.ok()) return outcome;

    auto event = services_.store->AppendEvent(
        task->project_id, "publication.finished",
        {{"task_id", task_id}, {"publication_id", outcome->publication_id},
         {"state", outcome->state}, {"applied", outcome->applied_paths.size()},
         {"failed", outcome->failed_paths.size()}});
    if (!event.ok()) {
        return Fail<workspace::PublicationOutcome>(event.code(), event.message());
    }

    if (outcome->state == "PUBLISHED") {
        services_.tasks->TransitionTask(task_id, tasks::TaskState::Validating);
    }
    return outcome;
}

Expected<std::vector<validation::ValidationResult>> Orchestrator::ValidateTask(
    const std::string& task_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto task = services_.tasks->GetTask(task_id);
    if (!task.ok()) {
        return Fail<std::vector<validation::ValidationResult>>(task.code(), task.message());
    }

    if (!services_.validation) {
        services_.tasks->TransitionTask(task_id, tasks::TaskState::Failed);
        return Fail<std::vector<validation::ValidationResult>>(
            ErrorCode::Internal, "validation engine unavailable; failing closed");
    }

    // Ensure the task is in Validating state (or transition to Validating if currently Integrating/Running)
    if (task->state != tasks::TaskState::Validating) {
        auto to_validating = services_.tasks->TransitionTask(task_id, tasks::TaskState::Validating);
        if (!to_validating.ok()) {
            return Fail<std::vector<validation::ValidationResult>>(
                to_validating.code(), "cannot transition task to Validating: " + to_validating.message());
        }
    }

    std::vector<validation::ValidationResult> verdicts;
    for (const auto& criterion : task->contract.criteria) {
        std::vector<evidence::EvidenceRecord> records;
        if (services_.evidence) {
            auto recs = services_.evidence->Records(criterion.criterion_id);
            if (recs.ok()) records = *recs;
        }
        auto res = services_.validation->Validate(criterion, records);
        if (res.ok()) {
            verdicts.push_back(*res);
        } else {
            validation::ValidationResult failed_result;
            failed_result.criterion_id = criterion.criterion_id;
            failed_result.check_id = "check:" + criterion.criterion_id;
            failed_result.check_version = "1";
            failed_result.verdict = validation::Verdict::Fail;
            failed_result.rationale = "validation execution failed: " + res.message();
            failed_result.failure_kind = validation::FailureKind::Unknown;
            verdicts.push_back(failed_result);
        }
    }

    // Fail-closed gate (AGENTS.md §§ 4.4, 13, 15):
    // An empty verdict set (no criteria or no results) represents missing/unexecuted checks and CANNOT pass.
    // Unknown or unverifiable state fails closed.
    const bool all_pass = !verdicts.empty() && std::all_of(
        verdicts.begin(), verdicts.end(), [](const validation::ValidationResult& v) {
            return v.verdict == validation::Verdict::Pass ||
                   v.verdict == validation::Verdict::NotApplicable;
        });

    if (all_pass) {
        auto transitioned = services_.tasks->TransitionTask(task_id, tasks::TaskState::Completed);
        if (!transitioned.ok()) {
            return Fail<std::vector<validation::ValidationResult>>(
                transitioned.code(), "failed transitioning task to Completed: " + transitioned.message());
        }
        services_.store->AppendEvent(
            task->project_id, "task.validation.passed",
            {{"task_id", task_id}, {"verdicts_count", verdicts.size()}});

        // Fire any registered triggers for ValidationPassed on this task or project
        auto triggers = TriggersLocked(task->project_id);
        for (const auto& trig : triggers) {
            if (!trig.fired && trig.kind == TriggerKind::ValidationPassed &&
                (trig.target_task_id.empty() || trig.target_task_id == task_id)) {
                FireTriggerLocked(trig.trigger_id);
            }
        }
    } else {
        services_.tasks->TransitionTask(task_id, tasks::TaskState::Failed);
        services_.store->AppendEvent(
            task->project_id, "task.validation.failed",
            {{"task_id", task_id},
             {"verdicts_count", verdicts.size()},
             {"reason", verdicts.empty() ? "empty validation verdicts: unverifiable state fails closed"
                                         : "one or more non-passing criteria"}});
    }

    return verdicts;
}

// --- Full council orchestration (FULL is the only mode) ----------------------------------

Expected<std::string> Orchestrator::OpenCouncilPoint(const std::string& project_id,
                                                     const std::string& topic,
                                                     const std::string& evidence_text,
                                                     const std::string& snapshot_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    council::CouncilPoint point;
    point.point_id = NewId("cpt");
    point.project_id = project_id;
    point.topic = topic;
    point.evidence = evidence_text;
    point.snapshot_id = snapshot_id;
    point.created_at = NowUtcIso8601();
    auto registered = services_.council->RegisterPoint(point);
    if (!registered.ok()) return Fail<std::string>(registered.code(), registered.message());

    auto event = services_.store->AppendEvent(
        project_id, "council.point.opened",
        {{"point_id", point.point_id}, {"topic", topic}, {"snapshot_id", snapshot_id}});
    if (!event.ok()) return Fail<std::string>(event.code(), event.message());
    return point.point_id;
}

Status Orchestrator::AdvanceCouncilRound(const std::string& point_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto state = services_.council->PointStateOf(point_id);
    if (!state.ok()) return state.status();
    const std::int64_t next = state->rounds.empty() ? 1 : state->rounds.back() + 1;
    return services_.council->BeginRound(point_id, next);
}

std::vector<Orchestrator::CouncilSessionLink> Orchestrator::CouncilSessions() {
    std::lock_guard<std::mutex> lock(mutex_);
    return council_sessions_;
}

Status Orchestrator::LinkCouncilSession(const CouncilSessionLink& link) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (link.session_id.empty() || link.point_id.empty()) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "council session link requires point_id and session_id");
    }
    council_sessions_.push_back(link);
    return Status::Ok();
}

Status Orchestrator::SubmitCouncilProposal(const std::string& point_id,
                                           const std::string& agent,
                                           const std::string& proposal_text,
                                           const nlohmann::json& evidence) {
    std::lock_guard<std::mutex> lock(mutex_);
    council::Proposal proposal;
    proposal.proposal_id = NewId("prop");
    proposal.point_id = point_id;
    proposal.agent = agent;
    proposal.position = "provisional";
    proposal.proposal = proposal_text;
    if (evidence.is_array()) {
        for (const auto& entry : evidence) proposal.evidence.push_back(entry);
    } else if (!evidence.is_null()) {
        proposal.evidence.push_back(evidence);
    }
    proposal.created_at = NowUtcIso8601();
    return services_.council->SubmitProposal(proposal);
}

Status Orchestrator::SubmitCouncilCritique(const std::string& point_id,
                                           const std::string& author,
                                           const std::string& target,
                                           const std::string& review) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto state = services_.council->PointStateOf(point_id);
    if (!state.ok()) return state.status();
    std::string target_proposal_id;
    for (const auto& proposal : state->proposals) {
        if (proposal.agent == target) target_proposal_id = proposal.proposal_id;
    }
    if (target_proposal_id.empty()) {
        return Status::Error(ErrorCode::NotFound,
                             "no proposal from agent " + target + " on point " + point_id);
    }
    council::Critique critique;
    critique.critique_id = NewId("crit");
    critique.proposal_id = target_proposal_id;
    critique.author_agent = author;
    critique.target_agent = target;
    critique.review = review;
    critique.created_at = NowUtcIso8601();
    return services_.council->SubmitCritique(critique);
}

Status Orchestrator::SubmitCouncilSynthesis(const std::string& point_id,
                                            const std::string& chair,
                                            const std::string& text,
                                            const std::string& nonchair_review,
                                            const std::vector<nlohmann::json>& disagreements) {
    std::lock_guard<std::mutex> lock(mutex_);
    council::Synthesis synthesis;
    synthesis.synthesis_id = NewId("syn");
    synthesis.point_id = point_id;
    synthesis.chair_agent = chair;
    synthesis.synthesized_resolution = text;
    synthesis.nonchair_review = nonchair_review;
    synthesis.disagreements = disagreements;
    synthesis.created_at = NowUtcIso8601();
    return services_.council->SubmitSynthesis(synthesis);
}

Expected<council::Engine::PointState> Orchestrator::CouncilPointState(const std::string& point_id) {
    return services_.council->PointStateOf(point_id);
}

// --- Registered triggers -----------------------------------------------------------------

Status Orchestrator::RegisterTrigger(const RegisteredTrigger& trigger) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto schema = EnsureTriggerSchema(services_.store);
    if (!schema.ok()) return schema;
    if (trigger.trigger_id.empty() || trigger.project_id.empty()) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "trigger requires trigger_id and project_id");
    }
    return services_.store->Exec(
        "INSERT OR REPLACE INTO orchestrator_triggers(trigger_id, kind, project_id,"
        " target_task_id, council_point_id, fired, created_at, fired_at)"
        " VALUES(?1, ?2, ?3, ?4, ?5, 0, ?6, '')",
        {SqlValue::Text(trigger.trigger_id), SqlValue::Text(TriggerKindName(trigger.kind)),
         SqlValue::Text(trigger.project_id), SqlValue::Text(trigger.target_task_id),
         SqlValue::Text(trigger.council_point_id),
         SqlValue::Text(trigger.created_at.empty() ? NowUtcIso8601() : trigger.created_at)});
}

Status Orchestrator::FireTriggerLocked(const std::string& trigger_id) {
    auto schema = EnsureTriggerSchema(services_.store);
    if (!schema.ok()) return schema;
    const std::string fired_at = NowUtcIso8601();
    auto updated = services_.store->Exec(
        "UPDATE orchestrator_triggers SET fired = 1, fired_at = ?2 WHERE trigger_id = ?1",
        {SqlValue::Text(trigger_id), SqlValue::Text(fired_at)});
    if (!updated.ok()) return updated;
    if (services_.store->Changes() == 0) {
        return Status::Error(ErrorCode::NotFound, "trigger not found: " + trigger_id);
    }
    return Status::Ok();
}

Status Orchestrator::FireTrigger(const std::string& trigger_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    return FireTriggerLocked(trigger_id);
}

std::vector<RegisteredTrigger> Orchestrator::TriggersLocked(const std::string& project_id) {
    std::vector<RegisteredTrigger> triggers;
    if (!EnsureTriggerSchema(services_.store).ok()) return triggers;
    auto rows = services_.store->Query(
        "SELECT trigger_id, kind, project_id, target_task_id, council_point_id, fired,"
        " created_at, fired_at FROM orchestrator_triggers WHERE project_id = ?1"
        " ORDER BY created_at",
        {SqlValue::Text(project_id)});
    if (!rows.ok()) return triggers;
    for (const auto& row : *rows) {
        RegisteredTrigger trigger;
        trigger.trigger_id = row.Text("trigger_id");
        trigger.project_id = row.Text("project_id");
        trigger.target_task_id = row.Text("target_task_id");
        trigger.council_point_id = row.Text("council_point_id");
        trigger.fired = row.Int("fired") != 0;
        trigger.created_at = row.Text("created_at");
        trigger.fired_at = row.Text("fired_at");
        const std::string kind = row.Text("kind");
        if (kind == "ExplorationComplete") trigger.kind = TriggerKind::ExplorationComplete;
        else if (kind == "CouncilPointResolved") trigger.kind = TriggerKind::CouncilPointResolved;
        else if (kind == "LeaseExpired") trigger.kind = TriggerKind::LeaseExpired;
        else if (kind == "ValidationPassed") trigger.kind = TriggerKind::ValidationPassed;
        else trigger.kind = TriggerKind::UserMilestone;
        triggers.push_back(trigger);
    }
    return triggers;
}

std::vector<RegisteredTrigger> Orchestrator::Triggers(const std::string& project_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    return TriggersLocked(project_id);
}

// --- Phase transitions -------------------------------------------------------------------

Status Orchestrator::TransitionPhase(const std::string& project_id, ProjectPhase phase) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto rows = services_.store->Query(
        "SELECT lifecycle_phase FROM projects WHERE project_id = ?1",
        {SqlValue::Text(project_id)});
    if (!rows.ok()) return rows.status();
    if (rows->empty()) {
        return Status::Error(ErrorCode::NotFound, "project not found: " + project_id);
    }
    const std::string current_name = rows->front().Text("lifecycle_phase");
    const auto current = ParseProjectPhase(current_name);
    if (!current.has_value()) {
        return Status::Error(ErrorCode::Conflict,
                             "stored lifecycle phase is not a recognized phase: " + current_name);
    }
    auto legal = ValidatePhaseTransition(*current, phase);
    if (!legal.ok()) {
        return Status::Error(legal.code(),
                             std::string("phase transition refused: ") + current_name + " -> " +
                                 ProjectPhaseName(phase) + ": " + legal.message());
    }
    auto updated = services_.store->Exec(
        "UPDATE projects SET lifecycle_phase = ?2, updated_at = ?3 WHERE project_id = ?1",
        {SqlValue::Text(project_id), SqlValue::Text(ProjectPhaseName(phase)),
         SqlValue::Text(NowUtcIso8601())});
    if (!updated.ok()) return updated;
    auto event = services_.store->AppendEvent(
        project_id, "project.phase.changed",
        {{"from", current_name}, {"to", ProjectPhaseName(phase)}});
    return event.ok() ? Status::Ok() : event.status();
}

}  // namespace mayasaba::app
