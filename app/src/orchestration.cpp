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
#include <set>
#include <sstream>
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

// --- Relative-path scope helpers (mirror the Task engine's conservative model) --------------

std::string NormalizeRel(const std::string& path) {
    std::string text = path;
    for (char& c : text) {
        if (c == '\\') c = '/';
    }
    std::transform(text.begin(), text.end(), text.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    while (text.rfind("./", 0) == 0) text = text.substr(2);
    while (!text.empty() && text.back() == '/') text.pop_back();
    return text;
}

// A wildcard/glob/placeholder path cannot be resolved to a concrete file: it is unknown.
bool IsUnknownScopePath(const std::string& normalized) {
    return normalized.find('*') != std::string::npos || normalized.find('?') != std::string::npos ||
           normalized.find('<') != std::string::npos || normalized.find('>') != std::string::npos;
}

bool ScopeAncestorOrEqual(const std::string& a, const std::string& b) {
    if (a == b) return true;
    if (a.empty() || a.size() >= b.size()) return false;
    if (b.compare(0, a.size(), a) != 0) return false;
    return b[a.size()] == '/';
}

// True when `rel` is covered by one of the concrete scope entries. An unknown (wildcard) scope
// entry is never a grant: unresolved authority must fail closed at publication too, not just at
// launch.
bool PathInScope(const std::string& rel, const std::vector<std::string>& scope) {
    const std::string normalized = NormalizeRel(rel);
    for (const auto& entry : scope) {
        const std::string candidate = NormalizeRel(entry);
        if (candidate.empty()) continue;
        if (IsUnknownScopePath(candidate)) continue;
        if (ScopeAncestorOrEqual(candidate, normalized)) return true;
    }
    return false;
}

bool ScopesOverlap(const std::string& left, const std::string& right) {
    const std::string a = NormalizeRel(left);
    const std::string b = NormalizeRel(right);
    if (a.empty() || b.empty()) return false;
    return ScopeAncestorOrEqual(a, b) || ScopeAncestorOrEqual(b, a);
}

std::string JoinPaths(const std::vector<std::string>& paths) {
    std::string joined;
    for (std::size_t i = 0; i < paths.size(); ++i) {
        if (i != 0) joined += ", ";
        joined += paths[i];
    }
    return joined;
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
        case ProjectPhase::Closed:
        case ProjectPhase::Abandoned:
            // Terminals: only explicit close/abandon transitions are valid.
            return to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok() : Status::Error(ErrorCode::Conflict, "terminal phase cannot transition");
        case ProjectPhase::Paused:
            // PAUSED is an orthogonal side condition (see the projects.condition column); a paused
            // project may resume to any active phase, or be closed/abandoned. It is not a terminal.
            return to == ProjectPhase::Bound || to == ProjectPhase::Exploration ||
                           to == ProjectPhase::Execution || to == ProjectPhase::Integrating ||
                           to == ProjectPhase::Validation || to == ProjectPhase::Completed ||
                           to == ProjectPhase::Closed || to == ProjectPhase::Abandoned
                       ? Status::Ok()
                       : Status::Error(ErrorCode::Conflict,
                                       "paused project cannot transition to this phase");
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
    if (validated.criteria.empty()) {
        return Fail<tasks::Task>(
            ErrorCode::InvalidArgument,
            "an actionable task contract requires at least one acceptance criterion (fail closed)");
    }

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
        if (task->state == tasks::TaskState::Pending) {
            // Surface the exact scheduling reason (dependency unmet / write conflict) instead of a
            // bare "not leasable" so the caller can report why progress is waiting.
            const auto reason = decision->reasons.find(task_id);
            return Fail<tasks::Lease>(
                ErrorCode::NotReady,
                "task " + task_id + " is not leasable: " +
                    (reason != decision->reasons.end() ? reason->second
                                                       : std::string("not ready per schedule")));
        }
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

    auto task = services_.tasks->GetTask(task_id);
    if (!task.ok()) return Fail<std::string>(task.code(), task.message());
    auto project = LoadProject(task->project_id);
    if (!project.ok()) return Fail<std::string>(project.code(), project.message());

    auto kind = adapters::ParseAgentId(agent);
    if (!kind.has_value()) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "unknown agent id: " + agent);
    }

    // --- Fencing + scope gates BEFORE any side effect (staging, grants, launch). -----------
    // A stale or foreign lease is rejected before launch: the acting attempt must still hold
    // the current lease version.
    auto lease = services_.tasks->ActiveLeaseFor(task_id);
    if (!lease.ok()) {
        return Fail<std::string>(ErrorCode::NotReady,
                                 "no active lease for task " + task_id +
                                     "; lease the task before starting execution");
    }
    if (!attempt_id.empty() && lease->attempt_id != attempt_id) {
        return Fail<std::string>(
            ErrorCode::Stale,
            "stale lease: attempt id " + attempt_id +
                " does not match the active lease attempt " + lease->attempt_id);
    }
    auto lease_valid = services_.tasks->ValidateLease(lease->lease_id, lease->version);
    if (!lease_valid.ok()) {
        return Fail<std::string>(lease_valid.code(),
                                 "lease rejected before launch: " + lease_valid.message());
    }

    // Default-deny write scope: a task with no declared or resolvable write scope is never
    // launched "unscoped". The scope is derived from the authorized request and the workspace
    // binding; unknown/wildcard or forbidden-overlapping paths are refused before any launch.
    const std::vector<std::string>& write_scope = task->contract.allowed_write_paths;
    if (write_scope.empty()) {
        return Fail<std::string>(
            ErrorCode::Denied,
            "task " + task_id +
                " refused: no write scope declared (default-deny); a scoped TaskContract with "
                "allowed_write_paths derived from the request scope is required before launch");
    }
    for (const auto& write_path : write_scope) {
        const std::string normalized = NormalizeRel(write_path);
        if (normalized.empty() || IsUnknownScopePath(normalized) ||
            !workspace::IsSafeRelativePath(write_path)) {
            return Fail<std::string>(
                ErrorCode::Denied,
                "task " + task_id + " refused: write scope path is unknown or unsafe: " +
                    write_path);
        }
    }
    for (const auto& write_path : write_scope) {
        for (const auto& forbidden : task->contract.forbidden_paths) {
            if (ScopesOverlap(write_path, forbidden)) {
                return Fail<std::string>(
                    ErrorCode::Denied,
                    "task " + task_id + " refused: write scope '" + write_path +
                        "' overlaps forbidden path '" + forbidden + "'");
            }
        }
    }

    auto staged = EnsureStaging(*task, *project);
    if (!staged.ok()) return Fail<std::string>(staged.code(), staged.message());
    auto scopes = GrantTaskScopes(*task, *project, staged->path);
    if (!scopes.ok()) return Fail<std::string>(scopes.code(), scopes.message());

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
        TaskSessionLink{task_id, attempt->attempt_id, lease->lease_id, lease->version,
                        staged->path};

    auto event = services_.store->AppendEvent(
        task->project_id, "task.execution.started",
        {{"task_id", task_id}, {"attempt_id", attempt->attempt_id}, {"agent", agent},
         {"session_id", session_id}, {"lease_version", lease->version}});
    if (!event.ok()) return Fail<std::string>(event.code(), event.message());
    return session_id;
}

Status Orchestrator::RouteSessionOutcomeLocked(const TaskSessionLink& link,
                                               const std::string& session_id,
                                               const std::string& outcome,
                                               const std::string& session_error,
                                               RouteOutcome* observed) {
    nlohmann::json result = {{"session_id", session_id},
                             {"session_error", session_error},
                             {"outcome", outcome}};
    const std::string attempt_status = outcome == "SUCCEEDED"  ? "SUCCEEDED"
                                       : outcome == "CANCELLED" ? "CANCELLED"
                                       : outcome == "UNKNOWN"   ? "UNKNOWN"
                                                                : "FAILED";
    auto finished = services_.tasks->FinishAttempt(link.attempt_id, attempt_status, result);

    auto task = services_.tasks->GetTask(link.task_id);
    if (!task.ok()) {
        if (observed) observed->detail = task.message();
        return finished.ok() ? task.status() : finished;
    }

    // Evidence: the observed session outcome is a controller observation, never acceptance
    // evidence. It proves what happened, not that the work is correct.
    if (services_.evidence) {
        evidence::EvidenceRecord draft;
        draft.project_id = task->project_id;
        draft.check_id = "agent-session-outcome";
        draft.check_version = "1";
        draft.artifact.path = link.staging_path;
        draft.artifact.kind = "DIRECTORY";
        draft.artifact.provenance = "agent:session:" + session_id;
        draft.artifact.collected_at = NowUtcIso8601();
        draft.outcome = outcome == "SUCCEEDED" ? "PASS"
                        : outcome == "FAILED" ? "FAIL"
                                              : "INCONCLUSIVE";
        draft.detail = session_error;
        draft.collector = "controller";
        draft.collected_at = NowUtcIso8601();
        services_.evidence->Collect(draft);   // integrity-checked at collection time
    }

    bool lease_revoked = false;
    std::string task_state = tasks::TaskStateName(task->state);
    Status transition = Status::Ok();

    if (outcome == "SUCCEEDED") {
        transition = services_.tasks->TransitionTask(link.task_id, tasks::TaskState::Integrating);
        if (transition.ok()) task_state = tasks::TaskStateName(tasks::TaskState::Integrating);
    } else if (outcome == "FAILED") {
        transition = services_.tasks->TransitionTask(link.task_id, tasks::TaskState::Failed);
        if (transition.ok()) task_state = tasks::TaskStateName(tasks::TaskState::Failed);
    } else if (outcome == "CANCELLED") {
        // Revoke the lease by compare-and-swap (fencing); a moved version is a conflict and the
        // lease is not touched. Cancellation is truthful even when the process outcome is not.
        if (!link.lease_id.empty()) {
            auto revoked = services_.tasks->RevokeLease(link.lease_id, link.lease_version);
            lease_revoked = revoked.ok();
        }
        transition = services_.tasks->TransitionTask(link.task_id, tasks::TaskState::Cancelled);
        if (transition.ok()) task_state = tasks::TaskStateName(tasks::TaskState::Cancelled);
    } else {
        // UNKNOWN: the process outcome could not be observed. Do NOT report success and do NOT
        // silently consume the retry — the attempt is UNKNOWN and the task is left for restart
        // recovery to reclaim. It never reaches Completed.
        task_state = std::string(tasks::TaskStateName(task->state)) + " (awaiting recovery)";
    }

    auto event = services_.store->AppendEvent(
        task->project_id, "task.execution.completed",
        {{"task_id", link.task_id}, {"attempt_id", link.attempt_id}, {"session_id", session_id},
         {"outcome", outcome}, {"attempt_status", attempt_status}, {"task_state", task_state},
         {"session_error", session_error}});

    if (observed) {
        observed->attempt_status = attempt_status;
        observed->task_state = task_state;
        observed->lease_revoked = lease_revoked;
        observed->detail = session_error;
    }
    if (!finished.ok()) return finished;
    if (!transition.ok()) return transition;
    if (!event.ok()) return event.status();
    return Status::Ok();
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

    // Prefer the observed gateway session state over the caller's summary: an UNKNOWN or
    // CANCELLED termination must never be reported as success nor as an ordinary failure.
    std::string outcome;
    gateway::SessionRecord record;
    const bool have_record = services_.gateway && services_.gateway->Session(session_id, &record);
    if (have_record && record.state == gateway::SessionState::Cancelled) {
        outcome = "CANCELLED";
    } else if (have_record && record.state == gateway::SessionState::Unknown) {
        outcome = "UNKNOWN";
    } else if (success) {
        outcome = "SUCCEEDED";
    } else {
        outcome = "FAILED";
    }
    return RouteSessionOutcomeLocked(link, session_id, outcome, session_error);
}

Status Orchestrator::OnAgentSessionOutcomeUnknown(const std::string& session_id,
                                                  const std::string& detail) {
    std::lock_guard<std::mutex> lock(mutex_);
    const auto it = task_sessions_.find(session_id);
    if (it == task_sessions_.end()) {
        return Status::Ok();   // not a task session
    }
    const TaskSessionLink link = it->second;
    task_sessions_.erase(it);
    return RouteSessionOutcomeLocked(link, session_id, "UNKNOWN", detail);
}

Expected<CancellationResult> Orchestrator::CancelTaskExecution(
    const std::string& session_id, const std::string& reason) {
    std::lock_guard<std::mutex> lock(mutex_);
    const auto it = task_sessions_.find(session_id);
    if (it == task_sessions_.end()) {
        return Fail<CancellationResult>(ErrorCode::NotFound,
                                        "no active task session: " + session_id);
    }
    const TaskSessionLink link = it->second;
    task_sessions_.erase(it);

    std::string observed;
    Status cancelled = Status::Ok();
    if (services_.gateway) {
        cancelled = services_.gateway->CancelSession(session_id, &observed);
    } else {
        cancelled = Status::Error(ErrorCode::Internal, "gateway unavailable");
    }

    // Determine the observed outcome from the gateway's post-cancel session state.
    std::string outcome = "UNKNOWN";
    gateway::SessionRecord record;
    if (services_.gateway && services_.gateway->Session(session_id, &record)) {
        if (record.state == gateway::SessionState::Cancelled ||
            record.state == gateway::SessionState::Failed) {
            outcome = "CANCELLED";
        } else if (record.state == gateway::SessionState::Unknown) {
            outcome = "UNKNOWN";
        }
    }

    RouteOutcome routed;
    auto route_status =
        RouteSessionOutcomeLocked(link, session_id, outcome, observed.empty() ? reason : observed,
                                  &routed);

    CancellationResult result;
    result.session_id = session_id;
    result.task_id = link.task_id;
    result.attempt_id = link.attempt_id;
    result.attempt_status = routed.attempt_status;
    result.task_state = routed.task_state;
    result.lease_revoked = routed.lease_revoked;
    result.detail = observed.empty() ? reason : observed;
    if (!cancelled.ok()) {
        result.detail += "; gateway cancel: " + cancelled.message();
    }
    if (!route_status.ok()) {
        result.detail += "; routing: " + route_status.message();
    }
    return result;
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
    auto candidate = workspace::BuildIntegrationCandidate(*staged, project->canonical_root);
    if (!candidate.ok()) {
        return Fail<workspace::IntegrationCandidate>(candidate.code(), candidate.message());
    }
    // Actual changed paths exceeding the assigned write scope are rejected, never retroactively
    // approved. Fail closed before anything is published.
    std::vector<std::string> out_of_scope;
    for (const auto& change : candidate->changes) {
        bool forbidden = false;
        for (const auto& forbidden_path : task->contract.forbidden_paths) {
            if (ScopesOverlap(change.rel_path, forbidden_path)) {
                forbidden = true;
                break;
            }
        }
        if (forbidden || !PathInScope(change.rel_path, task->contract.allowed_write_paths)) {
            out_of_scope.push_back(change.rel_path);
        }
    }
    if (!out_of_scope.empty()) {
        return Fail<workspace::IntegrationCandidate>(
            ErrorCode::Denied,
            "changed paths outside the task's authorized write scope: " +
                JoinPaths(out_of_scope));
    }
    return candidate;
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
    // Defense in depth: refuse to publish any changed path outside the task's authorized write
    // scope (never retroactively approved).
    std::vector<std::string> out_of_scope;
    for (const auto& change : candidate->changes) {
        bool forbidden = false;
        for (const auto& forbidden_path : task->contract.forbidden_paths) {
            if (ScopesOverlap(change.rel_path, forbidden_path)) {
                forbidden = true;
                break;
            }
        }
        if (forbidden || !PathInScope(change.rel_path, task->contract.allowed_write_paths)) {
            out_of_scope.push_back(change.rel_path);
        }
    }
    if (!out_of_scope.empty()) {
        return Fail<workspace::PublicationOutcome>(
            ErrorCode::Denied,
            "publication refused: changed paths outside the task's authorized write scope: " +
                JoinPaths(out_of_scope));
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

    // Durable orchestrator-owned publication marker for the validation ordering gate and restart
    // reconciliation (the authoritative per-file journal remains the Workspace Manager's).
    (void)services_.store->Exec(
        "INSERT OR REPLACE INTO orchestrator_publications(publication_id, task_id, state,"
        " created_at) VALUES(?1, ?2, ?3, ?4)",
        {SqlValue::Text(outcome->publication_id), SqlValue::Text(task_id),
         SqlValue::Text(outcome->state), SqlValue::Text(NowUtcIso8601())});

    if (outcome->state == "PUBLISHED") {
        services_.tasks->TransitionTask(task_id, tasks::TaskState::Validating);
    }
    return outcome;
}

bool Orchestrator::HasPublishedPublicationLocked(const std::string& task_id) {
    auto rows = services_.store->Query(
        "SELECT publication_id FROM orchestrator_publications WHERE task_id=?1 AND "
        "state='PUBLISHED' LIMIT 1",
        {SqlValue::Text(task_id)});
    return rows.ok() && !rows->empty();
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

    // Ordering gate (AGENTS.md §§ 4.4, 13): validation only follows a successful attempt and the
    // integration/publication stage. A task that never reached Integrating/Validating is refused
    // rather than validated, so publish-before-validate can never happen silently.
    if (task->state != tasks::TaskState::Validating &&
        task->state != tasks::TaskState::Integrating) {
        return Fail<std::vector<validation::ValidationResult>>(
            ErrorCode::Conflict,
            "validation refused: task " + task_id + " is " +
                tasks::TaskStateName(task->state) +
                "; a successful attempt and integration/publication must precede validation");
    }

    // Publish-before-validate: when the attempt produced staged changes, a PUBLISHED publication
    // must exist before validation. A no-change attempt needs no publication.
    if (task->state == tasks::TaskState::Integrating) {
        auto project = LoadProject(task->project_id);
        const auto staged_it = staging_.find(task_id);
        if (project.ok() && staged_it != staging_.end()) {
            auto candidate =
                workspace::BuildIntegrationCandidate(staged_it->second, project->canonical_root);
            if (candidate.ok() && !candidate->changes.empty() &&
                !HasPublishedPublicationLocked(task_id)) {
                return Fail<std::vector<validation::ValidationResult>>(
                    ErrorCode::Conflict,
                    "validation refused: task " + task_id +
                        " has unpublished staged changes; publish before validating");
            }
        }
    }

    // Ensure the task is in Validating state (from Integrating).
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
        validation::ValidationResult result;
        if (res.ok()) {
            result = *res;
        } else {
            result.criterion_id = criterion.criterion_id;
            result.check_id = "check:" + criterion.criterion_id;
            result.check_version = "1";
            result.verdict = validation::Verdict::Fail;
            result.rationale = "validation execution failed: " + res.message();
            result.failure_kind = validation::FailureKind::Unknown;
        }
        // A user_acceptance criterion is never auto-passed: an explicit user acceptance record
        // (collector == "user" with a PASS outcome) is required, otherwise it stays unresolved.
        if (criterion.oracle_class == "user_acceptance") {
            bool user_accepted = false;
            for (const auto& record : records) {
                if (record.collector == "user" && record.outcome == "PASS") {
                    user_accepted = true;
                    break;
                }
            }
            if (!user_accepted) {
                result.verdict = validation::Verdict::Inconclusive;
                result.rationale =
                    "user_acceptance criterion cannot be auto-passed; explicit user acceptance "
                    "evidence is required";
            }
        }
        verdicts.push_back(std::move(result));
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
    if (!services_.council) {
        return Status::Error(ErrorCode::Internal, "council service not initialized");
    }
    auto state = services_.council->PointStateOf(point_id);
    if (!state.ok()) return state.status();
    std::string target_proposal_id;
    for (auto it = state->proposals.rbegin(); it != state->proposals.rend(); ++it) {
        if (it->agent == target) {
            target_proposal_id = it->proposal_id;
            break;
        }
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

Status Orchestrator::SubmitCouncilSynthesis(const council::Synthesis& synthesis) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (!services_.council) {
        return Status::Error(ErrorCode::Internal, "council service not initialized");
    }
    council::Synthesis s = synthesis;
    if (s.synthesis_id.empty()) {
        s.synthesis_id = NewId("syn");
    }
    if (s.created_at.empty()) {
        s.created_at = NowUtcIso8601();
    }
    return services_.council->SubmitSynthesis(s);
}

Status Orchestrator::SubmitCouncilSynthesis(const std::string& point_id,
                                            const std::string& chair,
                                            const std::string& text,
                                            const std::string& review_agent,
                                            const std::string& nonchair_review,
                                            const std::vector<std::string>& cited_positions,
                                            const std::vector<nlohmann::json>& disagreements) {
    council::Synthesis synthesis;
    synthesis.synthesis_id = NewId("syn");
    synthesis.point_id = point_id;
    synthesis.chair_agent = chair;
    synthesis.synthesized_resolution = text;
    synthesis.review_agent = review_agent;
    synthesis.nonchair_review = nonchair_review;
    synthesis.disagreements = disagreements;
    synthesis.created_at = NowUtcIso8601();

    if (!cited_positions.empty()) {
        synthesis.cited_positions = cited_positions;
    } else if (services_.council) {
        auto survivors = services_.council->SurvivingProposals(point_id);
        if (survivors.ok() && !survivors.value().empty()) {
            for (const auto& survivor : survivors.value()) {
                synthesis.cited_positions.push_back(survivor.proposal_id);
            }
        }
    }

    return SubmitCouncilSynthesis(synthesis);
}

Status Orchestrator::SubmitCouncilSynthesis(const std::string& point_id,
                                            const std::string& chair,
                                            const std::string& text,
                                            const std::string& nonchair_review,
                                            const std::vector<nlohmann::json>& disagreements) {
    council::Synthesis synthesis;
    synthesis.synthesis_id = NewId("syn");
    synthesis.point_id = point_id;
    synthesis.chair_agent = chair;
    synthesis.synthesized_resolution = text;
    synthesis.nonchair_review = nonchair_review;
    synthesis.disagreements = disagreements;
    synthesis.created_at = NowUtcIso8601();
    return SubmitCouncilSynthesis(synthesis);
}

Expected<council::Engine::PointState> Orchestrator::CouncilPointState(const std::string& point_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (!services_.council) {
        return Fail<council::Engine::PointState>(ErrorCode::Internal,
                                                 "council service not initialized");
    }
    return services_.council->PointStateOf(point_id);
}

// --- Registered triggers -----------------------------------------------------------------

Status Orchestrator::RegisterTrigger(const RegisteredTrigger& trigger) {
    std::lock_guard<std::mutex> lock(mutex_);
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
    auto rows = services_.store->Query(
        "SELECT fired FROM orchestrator_triggers WHERE trigger_id = ?1",
        {SqlValue::Text(trigger_id)});
    if (!rows.ok()) return rows.status();
    if (rows->empty()) {
        return Status::Error(ErrorCode::NotFound, "trigger not found: " + trigger_id);
    }
    if (rows->front().Int("fired") != 0) {
        return Status::Ok();   // idempotent: a registered trigger fires exactly once
    }
    return services_.store->Exec(
        "UPDATE orchestrator_triggers SET fired = 1, fired_at = ?2 WHERE trigger_id = ?1 AND "
        "fired = 0",
        {SqlValue::Text(trigger_id), SqlValue::Text(NowUtcIso8601())});
}

Status Orchestrator::FireTrigger(const std::string& trigger_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    return FireTriggerLocked(trigger_id);
}

std::vector<RegisteredTrigger> Orchestrator::TriggersLocked(const std::string& project_id) {
    std::vector<RegisteredTrigger> triggers;
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
        "SELECT lifecycle_phase, condition FROM projects WHERE project_id = ?1",
        {SqlValue::Text(project_id)});
    if (!rows.ok()) return rows.status();
    if (rows->empty()) {
        return Status::Error(ErrorCode::NotFound, "project not found: " + project_id);
    }
    const std::string current_name = rows->front().Text("lifecycle_phase");
    const std::string current_condition = rows->front().Text("condition");
    const auto current = ParseProjectPhase(current_name);
    if (!current.has_value()) {
        return Status::Error(ErrorCode::Conflict,
                             "stored lifecycle phase is not a recognized phase: " + current_name);
    }

    // PAUSED and the other side conditions are orthogonal to the canonical lifecycle sequence:
    // pausing writes the condition column only, never the lifecycle phase, and never the epoch.
    if (phase == ProjectPhase::Paused) {
        auto paused = services_.store->Exec(
            "UPDATE projects SET condition = 'PAUSED', updated_at = ?2 WHERE project_id = ?1",
            {SqlValue::Text(project_id), SqlValue::Text(NowUtcIso8601())});
        if (!paused.ok()) return paused;
        auto event = services_.store->AppendEvent(
            project_id, "project.condition.changed",
            {{"from", current_condition}, {"to", "PAUSED"}, {"lifecycle_phase", current_name}});
        return event.ok() ? Status::Ok() : event.status();
    }

    auto legal = ValidatePhaseTransition(*current, phase);
    if (!legal.ok()) {
        return Status::Error(legal.code(),
                             std::string("phase transition refused: ") + current_name + " -> " +
                                 ProjectPhaseName(phase) + ": " + legal.message());
    }
    // Advancing the lifecycle clears an orthogonal pause/stop condition; the epoch is untouched
    // (only the Project service commits epoch changes).
    auto updated = services_.store->Exec(
        "UPDATE projects SET lifecycle_phase = ?2, condition = 'ACTIVE', updated_at = ?3 "
        "WHERE project_id = ?1",
        {SqlValue::Text(project_id), SqlValue::Text(ProjectPhaseName(phase)),
         SqlValue::Text(NowUtcIso8601())});
    if (!updated.ok()) return updated;
    auto event = services_.store->AppendEvent(
        project_id, "project.phase.changed",
        {{"from", current_name}, {"to", ProjectPhaseName(phase)}});
    return event.ok() ? Status::Ok() : event.status();
}

Status Orchestrator::SetProjectCondition(const std::string& project_id,
                                         const std::string& condition) {
    std::lock_guard<std::mutex> lock(mutex_);
    std::string upper;
    upper.reserve(condition.size());
    for (char c : condition) {
        upper.push_back(static_cast<char>(std::toupper(static_cast<unsigned char>(c))));
    }
    static const std::vector<std::string> kConditions = {"ACTIVE", "PAUSED", "STOPPED", "BLOCKED",
                                                         "RECOVERING"};
    if (std::find(kConditions.begin(), kConditions.end(), upper) == kConditions.end()) {
        return Status::Error(ErrorCode::InvalidArgument, "unknown project condition: " + condition);
    }
    auto rows = services_.store->Query("SELECT condition FROM projects WHERE project_id = ?1",
                                       {SqlValue::Text(project_id)});
    if (!rows.ok()) return rows.status();
    if (rows->empty()) {
        return Status::Error(ErrorCode::NotFound, "project not found: " + project_id);
    }
    const std::string from = rows->front().Text("condition");
    auto updated = services_.store->Exec(
        "UPDATE projects SET condition = ?2, updated_at = ?3 WHERE project_id = ?1",
        {SqlValue::Text(project_id), SqlValue::Text(upper), SqlValue::Text(NowUtcIso8601())});
    if (!updated.ok()) return updated;
    auto event = services_.store->AppendEvent(
        project_id, "project.condition.changed", {{"from", from}, {"to", upper}});
    return event.ok() ? Status::Ok() : event.status();
}

Expected<std::string> Orchestrator::ProjectCondition(const std::string& project_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto rows = services_.store->Query("SELECT condition FROM projects WHERE project_id = ?1",
                                       {SqlValue::Text(project_id)});
    if (!rows.ok()) return Fail<std::string>(rows.code(), rows.message());
    if (rows->empty()) {
        return Fail<std::string>(ErrorCode::NotFound, "project not found: " + project_id);
    }
    return rows->front().Text("condition");
}

// --- Synchronization barrier (state machine #8) ------------------------------------------

Expected<BarrierRecord> Orchestrator::LoadBarrierLocked(const std::string& barrier_id) {
    auto rows = services_.store->Query(
        "SELECT barrier_id, project_id, purpose, required_agents, context_digest, state,"
        " created_at, satisfied_at, expires_at_ms FROM orchestrator_sync_barriers"
        " WHERE barrier_id = ?1",
        {SqlValue::Text(barrier_id)});
    if (!rows.ok()) return Fail<BarrierRecord>(rows.code(), rows.message());
    if (rows->empty()) {
        return Fail<BarrierRecord>(ErrorCode::NotFound, "barrier not found: " + barrier_id);
    }
    const Row& row = rows->front();
    BarrierRecord record;
    record.barrier_id = row.Text("barrier_id");
    record.project_id = row.Text("project_id");
    record.purpose = row.Text("purpose");
    record.context_digest = row.Text("context_digest");
    record.state = row.Text("state");
    record.created_at = row.Text("created_at");
    record.satisfied_at = row.Text("satisfied_at");
    record.expires_at_ms = row.Int("expires_at_ms");
    auto parsed = nlohmann::json::parse(row.Text("required_agents"), nullptr, false);
    if (!parsed.is_discarded() && parsed.is_array()) {
        for (const auto& entry : parsed) {
            if (entry.is_string()) record.required_agents.push_back(entry.get<std::string>());
        }
    }
    return record;
}

Expected<std::string> Orchestrator::RegisterBarrier(
    const std::string& project_id, const std::string& purpose,
    const std::vector<std::string>& required_agents, const std::string& context_digest,
    std::int64_t ttl_ms) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (project_id.empty() || purpose.empty() || context_digest.empty()) {
        return Fail<std::string>(ErrorCode::InvalidArgument,
                                 "barrier requires project_id, purpose and context_digest");
    }
    std::vector<std::string> agents;
    for (const auto& agent : required_agents) {
        if (!adapters::ParseAgentId(agent).has_value()) {
            return Fail<std::string>(ErrorCode::InvalidArgument,
                                     "barrier requires known agent ids; unknown: " + agent);
        }
        if (std::find(agents.begin(), agents.end(), agent) == agents.end()) {
            agents.push_back(agent);
        }
    }
    if (agents.empty()) {
        return Fail<std::string>(ErrorCode::InvalidArgument,
                                 "barrier requires at least one required agent");
    }

    nlohmann::json required = nlohmann::json::array();
    for (const auto& agent : agents) required.push_back(agent);

    BarrierRecord record;
    record.barrier_id = NewId("bar");
    record.project_id = project_id;
    record.purpose = purpose;
    record.required_agents = agents;
    record.context_digest = context_digest;
    record.state = "waiting";
    record.created_at = NowUtcIso8601();
    record.expires_at_ms = ttl_ms > 0 ? UnixTimeMillis() + ttl_ms : 0;

    auto inserted = services_.store->Exec(
        "INSERT INTO orchestrator_sync_barriers(barrier_id, project_id, purpose, required_agents,"
        " context_digest, state, created_at, satisfied_at, expires_at_ms)"
        " VALUES(?1, ?2, ?3, ?4, ?5, 'waiting', ?6, '', ?7)",
        {SqlValue::Text(record.barrier_id), SqlValue::Text(project_id), SqlValue::Text(purpose),
         SqlValue::Text(required.dump()), SqlValue::Text(context_digest),
         SqlValue::Text(record.created_at), SqlValue::Int(record.expires_at_ms)});
    if (!inserted.ok()) return Fail<std::string>(inserted.code(), inserted.message());

    auto event = services_.store->AppendEvent(
        project_id, "barrier.registered",
        {{"barrier_id", record.barrier_id}, {"purpose", purpose},
         {"required_agents", required}, {"context_digest", context_digest}});
    if (!event.ok()) return Fail<std::string>(event.code(), event.message());
    return record.barrier_id;
}

Status Orchestrator::AcknowledgeBarrier(const std::string& barrier_id, const std::string& agent,
                                        const std::string& context_digest) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto loaded = LoadBarrierLocked(barrier_id);
    if (!loaded.ok()) return loaded.status();
    BarrierRecord record = *loaded;

    if (record.state == "expired") {
        return Status::Error(ErrorCode::Conflict, "barrier expired; acknowledgement not counted");
    }
    if (record.state == "satisfied") {
        // Idempotent for a repeated identical acknowledgement; otherwise the barrier is closed.
        auto existing = services_.store->Query(
            "SELECT context_digest FROM orchestrator_sync_acks WHERE barrier_id=?1 AND agent=?2",
            {SqlValue::Text(barrier_id), SqlValue::Text(agent)});
        if (existing.ok() && !existing->empty() &&
            existing->front().Text("context_digest") == record.context_digest) {
            return Status::Ok();
        }
        return Status::Error(ErrorCode::Conflict,
                             "barrier already satisfied; acknowledgement not counted");
    }
    if (record.expires_at_ms > 0 && UnixTimeMillis() >= record.expires_at_ms) {
        services_.store->Exec(
            "UPDATE orchestrator_sync_barriers SET state='expired' WHERE barrier_id=?1",
            {SqlValue::Text(barrier_id)});
        return Status::Error(ErrorCode::Conflict, "barrier expired; acknowledgement not counted");
    }
    if (std::find(record.required_agents.begin(), record.required_agents.end(), agent) ==
        record.required_agents.end()) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "agent is not required for this barrier: " + agent);
    }
    if (context_digest != record.context_digest) {
        // Digest mismatch: rejected and never counted (it does not contribute to satisfaction).
        return Status::Error(ErrorCode::Stale,
                             "context digest mismatch; acknowledgement not counted");
    }

    auto ack = services_.store->Exec(
        "INSERT OR IGNORE INTO orchestrator_sync_acks(barrier_id, agent, context_digest,"
        " acknowledged_at) VALUES(?1, ?2, ?3, ?4)",
        {SqlValue::Text(barrier_id), SqlValue::Text(agent), SqlValue::Text(context_digest),
         SqlValue::Text(NowUtcIso8601())});
    if (!ack.ok()) return ack;

    // Satisfied only when EVERY required agent acknowledged the SAME digest.
    auto counted = services_.store->Query(
        "SELECT agent FROM orchestrator_sync_acks WHERE barrier_id=?1 AND context_digest=?2",
        {SqlValue::Text(barrier_id), SqlValue::Text(record.context_digest)});
    if (!counted.ok()) return counted.status();
    std::set<std::string> acked;
    for (const auto& row : *counted) acked.insert(row.Text("agent"));
    bool all = true;
    for (const auto& required_agent : record.required_agents) {
        if (acked.find(required_agent) == acked.end()) {
            all = false;
            break;
        }
    }
    if (all) {
        auto updated = services_.store->Exec(
            "UPDATE orchestrator_sync_barriers SET state='satisfied', satisfied_at=?2"
            " WHERE barrier_id=?1",
            {SqlValue::Text(barrier_id), SqlValue::Text(NowUtcIso8601())});
        if (!updated.ok()) return updated;
        services_.store->AppendEvent(
            record.project_id, "barrier.satisfied",
            {{"barrier_id", barrier_id}, {"context_digest", record.context_digest}});
    }
    return Status::Ok();
}

Expected<BarrierRecord> Orchestrator::BarrierState(const std::string& barrier_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto loaded = LoadBarrierLocked(barrier_id);
    if (!loaded.ok()) return loaded;
    BarrierRecord record = *loaded;

    // Timeout is reported, never auto-satisfied.
    if (record.state == "waiting" && record.expires_at_ms > 0 &&
        UnixTimeMillis() >= record.expires_at_ms) {
        services_.store->Exec(
            "UPDATE orchestrator_sync_barriers SET state='expired' WHERE barrier_id=?1",
            {SqlValue::Text(barrier_id)});
        record.state = "expired";
    }
    auto acks = services_.store->Query(
        "SELECT agent FROM orchestrator_sync_acks WHERE barrier_id=?1 AND context_digest=?2"
        " ORDER BY agent ASC",
        {SqlValue::Text(barrier_id), SqlValue::Text(record.context_digest)});
    if (acks.ok()) {
        for (const auto& row : *acks) {
            const std::string acked_agent = row.Text("agent");
            if (std::find(record.required_agents.begin(), record.required_agents.end(),
                          acked_agent) != record.required_agents.end()) {
                record.acknowledged_agents.push_back(acked_agent);
            }
        }
    }
    return record;
}

// --- Restart recovery --------------------------------------------------------------------

Expected<RecoveryReport> Orchestrator::RecoverAfterRestart(const std::string& project_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    RecoveryReport report;

    // Reclaim stale/expired leases: task -> Ready, interrupted attempt -> UNKNOWN. Idempotent:
    // an already-reclaimed lease is not expired again and no retry is double-consumed.
    auto expired = services_.tasks->ExpireStaleLeases(project_id);
    if (!expired.ok()) {
        return Fail<RecoveryReport>(expired.code(), expired.message());
    }
    report.expired_lease_tasks = *expired;

    // Reconcile pending publication journals truthfully (never rolls back newer user edits).
    auto project = LoadProject(project_id);
    if (!project.ok()) {
        report.details.push_back("project unavailable: " + project.message());
        return report;
    }
    auto pending = workspace::PendingPublications(services_.store);
    if (!pending.ok()) {
        report.details.push_back("pending publications unavailable: " + pending.message());
        return report;
    }
    for (const auto& entry : *pending) {
        if (entry.value("project_id", std::string()) != project_id) continue;
        const std::string publication_id = entry.value("publication_id", std::string());
        if (publication_id.empty()) continue;
        auto reconciled =
            workspace::Reconcile(publication_id, services_.store, project->canonical_root);
        report.reconciled_publications.push_back(publication_id);
        if (reconciled.ok()) {
            report.details.push_back("publication " + publication_id + " -> " + reconciled->state);
        } else {
            report.details.push_back("publication " + publication_id +
                                     " reconcile failed: " + reconciled.message());
        }
    }
    return report;
}

}  // namespace mayasaba::app
