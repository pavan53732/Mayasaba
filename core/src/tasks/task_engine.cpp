// Task/DAG Engine (layer 7) implementation.
//
// Ownership: this service owns task graphs, contracts, attempts, leases with fencing versions,
// dependency- and conflict-aware scheduling, and attempt evidence bundles (AGENTS.md 4.2).
//
// Persistence consolidation note: the frozen header gives this service a storage::Store* and the
// task brief asks the service to create its own tables lazily. The tables below are therefore
// created by EnsureSchema() with CREATE TABLE IF NOT EXISTS and are namespaced with the
// `task_engine_` prefix so they never collide with the storage layer's migration registry
// (core/src/storage/store.cpp already defines similarly-shaped `tasks`/`leases`/`attempts`
// tables). A future consolidation should move these definitions into the storage owner's
// migration registry so a single owner defines the schema.
#include "mayasaba/tasks.hpp"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::tasks {
namespace {

// --- Small shared helpers ------------------------------------------------------------------

const std::vector<std::string>& KnownAgents() {
    static const std::vector<std::string> kAgents = {"hermes", "kilo", "claude"};
    return kAgents;
}

bool IsKnownAgent(const std::string& agent) {
    const auto& agents = KnownAgents();
    return std::find(agents.begin(), agents.end(), agent) != agents.end();
}

const std::vector<std::string>& KnownOracleClasses() {
    static const std::vector<std::string> kClasses = {
        "policy",       "artifact_integrity", "static",       "compile",
        "unit",         "integration",        "runtime",      "e2e",
        "data_integrity", "citation",         "user_acceptance"};
    return kClasses;
}

bool IsKnownOracleClass(const std::string& oracle_class) {
    const auto& classes = KnownOracleClasses();
    return std::find(classes.begin(), classes.end(), oracle_class) != classes.end();
}

const std::vector<std::string>& KnownAttemptStatuses() {
    static const std::vector<std::string> kStatuses = {"STARTED", "RUNNING",  "SUCCEEDED",
                                                       "FAILED",  "CANCELLED", "UNKNOWN"};
    return kStatuses;
}

bool IsKnownAttemptStatus(const std::string& status) {
    const auto& statuses = KnownAttemptStatuses();
    return std::find(statuses.begin(), statuses.end(), status) != statuses.end();
}

std::string LowerAscii(std::string text) {
    std::transform(text.begin(), text.end(), text.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return text;
}

// --- Schema (lazy, idempotent) -------------------------------------------------------------

Status EnsureSchema(storage::Store* store) {
    static const char* kDdl[] = {
        "CREATE TABLE IF NOT EXISTS task_engine_tasks("
        "task_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, title TEXT NOT NULL, "
        "contract TEXT NOT NULL, state TEXT NOT NULL, assigned_agent TEXT NOT NULL DEFAULT '', "
        "created_at TEXT NOT NULL, updated_at TEXT NOT NULL);",
        "CREATE INDEX IF NOT EXISTS idx_task_engine_tasks_project "
        "ON task_engine_tasks(project_id);",
        "CREATE TABLE IF NOT EXISTS task_engine_deps("
        "task_id TEXT NOT NULL, depends_on TEXT NOT NULL, PRIMARY KEY(task_id, depends_on));",
        "CREATE TABLE IF NOT EXISTS task_engine_leases("
        "lease_id TEXT PRIMARY KEY, task_id TEXT NOT NULL, attempt_id TEXT NOT NULL, "
        "version INTEGER NOT NULL DEFAULT 1, state TEXT NOT NULL DEFAULT 'ACTIVE', "
        "issued_at TEXT NOT NULL, expires_at TEXT, expires_at_ms INTEGER NOT NULL DEFAULT 0, "
        "revoked_at TEXT, allowed_reads TEXT NOT NULL DEFAULT '[]', "
        "allowed_writes TEXT NOT NULL DEFAULT '[]');",
        "CREATE INDEX IF NOT EXISTS idx_task_engine_leases_task "
        "ON task_engine_leases(task_id, state);",
        "CREATE TABLE IF NOT EXISTS task_engine_attempts("
        "attempt_id TEXT PRIMARY KEY, task_id TEXT NOT NULL, agent TEXT NOT NULL, "
        "status TEXT NOT NULL, lease_version INTEGER NOT NULL DEFAULT 0, "
        "session_id TEXT NOT NULL DEFAULT '', baseline TEXT NOT NULL DEFAULT '{}', "
        "result TEXT NOT NULL DEFAULT '{}', started_at TEXT NOT NULL, ended_at TEXT);",
        "CREATE INDEX IF NOT EXISTS idx_task_engine_attempts_task "
        "ON task_engine_attempts(task_id);",
        "CREATE TABLE IF NOT EXISTS task_engine_attempt_evidence("
        "bundle_id TEXT PRIMARY KEY, task_id TEXT NOT NULL, attempt_id TEXT NOT NULL, "
        "lease_id TEXT NOT NULL, lease_version INTEGER NOT NULL DEFAULT 0, "
        "agent TEXT NOT NULL DEFAULT '', session_id TEXT NOT NULL DEFAULT '', "
        "baseline TEXT NOT NULL DEFAULT '{}', observed_changes TEXT NOT NULL DEFAULT '[]', "
        "process_outcomes TEXT NOT NULL DEFAULT '{}', diagnostics TEXT NOT NULL DEFAULT '[]', "
        "proposed_criterion_evidence TEXT NOT NULL DEFAULT '[]', "
        "known_omissions TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL);",
    };
    for (const char* sql : kDdl) {
        auto status = store->Exec(sql);
        if (!status.ok()) return status;
    }
    return Status::Ok();
}

// --- JSON helpers --------------------------------------------------------------------------

nlohmann::json CriterionToJson(const AcceptanceCriterion& criterion) {
    return {
        {"criterion_id", criterion.criterion_id},
        {"requirement_id", criterion.requirement_id},
        {"expectation", criterion.expectation},
        {"oracle_class", criterion.oracle_class},
        {"oracle_spec", criterion.oracle_spec},
        {"blocking", criterion.blocking},
        {"evidence_scope", criterion.evidence_scope},
    };
}

AcceptanceCriterion CriterionFromJson(const nlohmann::json& value) {
    AcceptanceCriterion criterion;
    criterion.criterion_id = value.value("criterion_id", std::string());
    criterion.requirement_id = value.value("requirement_id", std::string());
    criterion.expectation = value.value("expectation", std::string());
    criterion.oracle_class = value.value("oracle_class", std::string());
    criterion.oracle_spec = value.value("oracle_spec", std::string());
    criterion.blocking = value.value("blocking", true);
    criterion.evidence_scope = value.value("evidence_scope", std::string());
    return criterion;
}

Expected<std::vector<std::string>> StringArrayFromJson(const nlohmann::json& value,
                                                       const std::string& field) {
    std::vector<std::string> out;
    if (value.is_null()) return out;
    if (!value.is_array()) {
        return Fail<std::vector<std::string>>(ErrorCode::SchemaViolation,
                                              "field is not an array: " + field);
    }
    for (const auto& item : value) {
        if (!item.is_string()) {
            return Fail<std::vector<std::string>>(ErrorCode::SchemaViolation,
                                                  "array element is not a string: " + field);
        }
        out.push_back(item.get<std::string>());
    }
    return out;
}

nlohmann::json StringArrayToJson(const std::vector<std::string>& values) {
    nlohmann::json array = nlohmann::json::array();
    for (const auto& value : values) array.push_back(value);
    return array;
}

// --- Task row mapping ----------------------------------------------------------------------

Expected<Task> RowToTask(const storage::Row& row) {
    Task task;
    task.task_id = row.Text("task_id");
    task.project_id = row.Text("project_id");
    task.title = row.Text("title");
    task.assigned_agent = row.Text("assigned_agent");
    task.created_at = row.Text("created_at");
    task.updated_at = row.Text("updated_at");
    task.state = ParseTaskState(row.Text("state")).value_or(TaskState::Pending);
    auto parsed = ParseJsonBounded(row.Text("contract"));
    if (!parsed.ok()) {
        return Fail<Task>(ErrorCode::IntegrityFailure, "stored task contract is unreadable");
    }
    auto contract = TaskContract::FromJson(parsed.value());
    if (!contract.ok()) return Expected<Task>(contract.status());
    task.contract = contract.take();
    return task;
}

// --- Write-set conflict model --------------------------------------------------------------

std::string NormalizePath(const std::string& path) {
    std::string text = path;
    for (char& c : text) {
        if (c == '\\') c = '/';
    }
    text = LowerAscii(text);
    while (text.rfind("./", 0) == 0) text = text.substr(2);
    while (!text.empty() && text.back() == '/') text.pop_back();
    return text;
}

// A path that cannot be resolved to a concrete file/directory (wildcard, glob, placeholder) is
// treated as unknown and conservatively conflicts with everything.
bool IsUnknownPath(const std::string& normalized) {
    return normalized.find('*') != std::string::npos || normalized.find('?') != std::string::npos ||
           normalized.find('<') != std::string::npos || normalized.find('>') != std::string::npos;
}

bool IsAncestorOrEqual(const std::string& a, const std::string& b) {
    if (a == b) return true;
    if (a.empty() || a.size() >= b.size()) return false;
    if (b.compare(0, a.size(), a) != 0) return false;
    return b[a.size()] == '/';
}

// Conservative conflict rule (documented in the report): an empty or unknown write set conflicts
// with every other write set, including another empty one. Two concrete, non-overlapping write
// sets do not conflict.
bool WriteSetsConflict(const std::vector<std::string>& left,
                       const std::vector<std::string>& right) {
    if (left.empty() || right.empty()) return true;
    std::vector<std::string> normalized_left;
    std::vector<std::string> normalized_right;
    for (const auto& path : left) normalized_left.push_back(NormalizePath(path));
    for (const auto& path : right) normalized_right.push_back(NormalizePath(path));
    for (const auto& path : normalized_left) {
        if (IsUnknownPath(path)) return true;
    }
    for (const auto& path : normalized_right) {
        if (IsUnknownPath(path)) return true;
    }
    for (const auto& a : normalized_left) {
        for (const auto& b : normalized_right) {
            if (IsAncestorOrEqual(a, b) || IsAncestorOrEqual(b, a)) return true;
        }
    }
    return false;
}

// --- Legal task-state transition set -------------------------------------------------------
// Documented in the report. Terminal states (Completed, Failed, Cancelled) accept nothing.
bool IsLegalTransition(TaskState from, TaskState to) {
    if (from == to) return false;
    switch (from) {
        case TaskState::Pending:
            return to == TaskState::Ready || to == TaskState::Blocked ||
                   to == TaskState::Cancelled;
        case TaskState::Ready:
            return to == TaskState::Leased || to == TaskState::Blocked ||
                   to == TaskState::Cancelled;
        case TaskState::Leased:
            return to == TaskState::Running || to == TaskState::Ready ||
                   to == TaskState::Failed || to == TaskState::Cancelled;
        case TaskState::Running:
            return to == TaskState::Integrating || to == TaskState::Validating ||
                   to == TaskState::Completed || to == TaskState::Failed ||
                   to == TaskState::Cancelled;
        case TaskState::Integrating:
            return to == TaskState::Validating || to == TaskState::Completed ||
                   to == TaskState::Failed;
        case TaskState::Validating:
            return to == TaskState::Completed || to == TaskState::Failed ||
                   to == TaskState::Integrating;
        case TaskState::Blocked:
            return to == TaskState::Ready || to == TaskState::Pending ||
                   to == TaskState::Cancelled;
        case TaskState::Completed:
        case TaskState::Failed:
        case TaskState::Cancelled:
            return false;
    }
    return false;
}

// Depth-first reachability over the dependency graph (task_id -> depends_on).
bool DependsTransitivelyOn(storage::Store* store, const std::string& from,
                           const std::string& target) {
    std::set<std::string> visited;
    std::vector<std::string> stack{from};
    while (!stack.empty()) {
        std::string current = stack.back();
        stack.pop_back();
        if (current == target) return true;
        if (!visited.insert(current).second) continue;
        auto rows = store->Query("SELECT depends_on FROM task_engine_deps WHERE task_id=?;",
                                 {storage::SqlValue::Text(current)});
        if (!rows.ok()) continue;
        for (const auto& row : rows.value()) stack.push_back(row.Text("depends_on"));
    }
    return false;
}

}  // namespace

// --- TaskContract JSON ---------------------------------------------------------------------

nlohmann::json TaskContract::ToJson() const {
    nlohmann::json criteria_json = nlohmann::json::array();
    for (const auto& criterion : criteria) criteria_json.push_back(CriterionToJson(criterion));
    return {
        {"task_id", task_id},
        {"project_id", project_id},
        {"work_request_id", work_request_id},
        {"contribution_id", contribution_id},
        {"requirement_id", requirement_id},
        {"decision_id", decision_id},
        {"epoch", epoch},
        {"snapshot_id", snapshot_id},
        {"context_digest", context_digest},
        {"objective", objective},
        {"expected_artifacts", StringArrayToJson(expected_artifacts)},
        {"dependencies", StringArrayToJson(dependencies)},
        {"assigned_agent", assigned_agent},
        {"allowed_read_paths", StringArrayToJson(allowed_read_paths)},
        {"allowed_write_paths", StringArrayToJson(allowed_write_paths)},
        {"forbidden_paths", StringArrayToJson(forbidden_paths)},
        {"approved_capabilities", StringArrayToJson(approved_capabilities)},
        {"time_budget_ms", time_budget_ms},
        {"token_budget", token_budget},
        {"expected_change_types", StringArrayToJson(expected_change_types)},
        {"criteria", criteria_json},
    };
}

Expected<TaskContract> TaskContract::FromJson(const nlohmann::json& value) {
    if (!value.is_object()) {
        return Fail<TaskContract>(ErrorCode::SchemaViolation, "task contract must be an object");
    }
    TaskContract contract;
    try {
        contract.task_id = value.value("task_id", std::string());
        contract.project_id = value.value("project_id", std::string());
        contract.work_request_id = value.value("work_request_id", std::string());
        contract.contribution_id = value.value("contribution_id", std::string());
        contract.requirement_id = value.value("requirement_id", std::string());
        contract.decision_id = value.value("decision_id", std::string());
        contract.epoch = value.value("epoch", static_cast<std::int64_t>(0));
        contract.snapshot_id = value.value("snapshot_id", std::string());
        contract.context_digest = value.value("context_digest", std::string());
        contract.objective = value.value("objective", std::string());
        contract.assigned_agent = value.value("assigned_agent", std::string());
        contract.time_budget_ms = value.value("time_budget_ms", static_cast<std::int64_t>(0));
        contract.token_budget = value.value("token_budget", static_cast<std::int64_t>(0));

        struct ArrayField {
            const char* name;
            std::vector<std::string>* target;
        };
        const ArrayField kArrayFields[] = {
            {"expected_artifacts", &contract.expected_artifacts},
            {"dependencies", &contract.dependencies},
            {"allowed_read_paths", &contract.allowed_read_paths},
            {"allowed_write_paths", &contract.allowed_write_paths},
            {"forbidden_paths", &contract.forbidden_paths},
            {"approved_capabilities", &contract.approved_capabilities},
            {"expected_change_types", &contract.expected_change_types},
        };
        for (const auto& field : kArrayFields) {
            if (!value.contains(field.name)) continue;
            auto parsed = StringArrayFromJson(value[field.name], field.name);
            if (!parsed.ok()) return Fail<TaskContract>(parsed.code(), parsed.message());
            *field.target = parsed.take();
        }

        if (value.contains("criteria")) {
            const auto& criteria_value = value["criteria"];
            if (!criteria_value.is_array()) {
                return Fail<TaskContract>(ErrorCode::SchemaViolation,
                                          "criteria is not an array");
            }
            for (const auto& item : criteria_value) {
                if (!item.is_object()) {
                    return Fail<TaskContract>(ErrorCode::SchemaViolation,
                                              "criterion is not an object");
                }
                contract.criteria.push_back(CriterionFromJson(item));
            }
        }
    } catch (const nlohmann::json::exception& error) {
        return Fail<TaskContract>(ErrorCode::SchemaViolation,
                                  std::string("contract field type error: ") + error.what());
    }
    return contract;
}

// --- Task state names ----------------------------------------------------------------------

const char* TaskStateName(TaskState state) {
    switch (state) {
        case TaskState::Pending: return "pending";
        case TaskState::Ready: return "ready";
        case TaskState::Leased: return "leased";
        case TaskState::Running: return "running";
        case TaskState::Integrating: return "integrating";
        case TaskState::Validating: return "validating";
        case TaskState::Completed: return "completed";
        case TaskState::Blocked: return "blocked";
        case TaskState::Failed: return "failed";
        case TaskState::Cancelled: return "cancelled";
    }
    return "unknown";
}

std::optional<TaskState> ParseTaskState(const std::string& name) {
    const std::string lowered = LowerAscii(name);
    if (lowered == "pending") return TaskState::Pending;
    if (lowered == "ready") return TaskState::Ready;
    if (lowered == "leased") return TaskState::Leased;
    if (lowered == "running") return TaskState::Running;
    if (lowered == "integrating") return TaskState::Integrating;
    if (lowered == "validating") return TaskState::Validating;
    if (lowered == "completed") return TaskState::Completed;
    if (lowered == "blocked") return TaskState::Blocked;
    if (lowered == "failed") return TaskState::Failed;
    if (lowered == "cancelled" || lowered == "canceled") return TaskState::Cancelled;
    return std::nullopt;
}

// --- Contract validation -------------------------------------------------------------------

Status ValidateContract(const TaskContract& contract) {
    if (contract.task_id.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "task contract requires task_id");
    }
    if (contract.project_id.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "task contract requires project_id");
    }
    if (contract.objective.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "task contract requires an objective");
    }
    if (!contract.assigned_agent.empty() && !IsKnownAgent(contract.assigned_agent)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "assigned_agent is not a registered agent: " +
                                 contract.assigned_agent);
    }
    for (const auto& criterion : contract.criteria) {
        if (criterion.criterion_id.empty()) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "acceptance criterion requires criterion_id");
        }
        if (!IsKnownOracleClass(criterion.oracle_class)) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "acceptance criterion '" + criterion.criterion_id +
                                     "' has unknown oracle_class: " + criterion.oracle_class);
        }
        if (criterion.expectation.empty()) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "acceptance criterion '" + criterion.criterion_id +
                                     "' has an empty expectation");
        }
    }
    // A task that declares artifacts to produce is a blocking task: it must carry at least one
    // blocking acceptance criterion, otherwise there is no acceptance gate to enforce.
    if (!contract.expected_artifacts.empty()) {
        bool has_blocking = false;
        for (const auto& criterion : contract.criteria) {
            if (criterion.blocking) {
                has_blocking = true;
                break;
            }
        }
        if (!has_blocking) {
            return Status::Error(
                ErrorCode::InvalidArgument,
                "blocking task requires at least one blocking acceptance criterion");
        }
    }
    return Status::Ok();
}

// --- Engine: registration and reads --------------------------------------------------------

Expected<Task> Engine::RegisterTask(const std::string& title, const TaskContract& contract) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<Task>(schema);
    auto valid = ValidateContract(contract);
    if (!valid.ok()) return Expected<Task>(valid);
    if (title.empty()) {
        return Fail<Task>(ErrorCode::InvalidArgument, "task requires a title");
    }
    auto existing = store_->Query("SELECT task_id FROM task_engine_tasks WHERE task_id=?;",
                                  {storage::SqlValue::Text(contract.task_id)});
    if (!existing.ok()) return Expected<Task>(existing.status());
    if (!existing.value().empty()) {
        return Fail<Task>(ErrorCode::AlreadyExists, "task already registered: " + contract.task_id);
    }

    Task task;
    task.task_id = contract.task_id;
    task.project_id = contract.project_id;
    task.title = title;
    task.contract = contract;
    task.state = TaskState::Pending;
    task.assigned_agent = contract.assigned_agent;
    task.created_at = NowUtcIso8601();
    task.updated_at = task.created_at;

    bool own_transaction = !store_->InTransaction();
    std::optional<storage::Transaction> guard;
    if (own_transaction) {
        auto begun = store_->Begin();
        if (!begun.ok()) return Expected<Task>(begun.status());
        guard.emplace(std::move(begun.value()));
    }
    auto insert = store_->Exec(
        "INSERT INTO task_engine_tasks(task_id, project_id, title, contract, state, "
        "assigned_agent, created_at, updated_at) VALUES(?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(task.task_id), storage::SqlValue::Text(task.project_id),
         storage::SqlValue::Text(task.title), storage::SqlValue::Text(contract.ToJson().dump()),
         storage::SqlValue::Text(TaskStateName(task.state)),
         storage::SqlValue::Text(task.assigned_agent), storage::SqlValue::Text(task.created_at),
         storage::SqlValue::Text(task.updated_at)});
    if (!insert.ok()) return Expected<Task>(insert);
    for (const auto& dependency : contract.dependencies) {
        if (dependency.empty() || dependency == task.task_id) continue;
        auto dep = store_->Exec(
            "INSERT OR IGNORE INTO task_engine_deps(task_id, depends_on) VALUES(?,?);",
            {storage::SqlValue::Text(task.task_id), storage::SqlValue::Text(dependency)});
        if (!dep.ok()) return Expected<Task>(dep);
    }
    auto event = store_->AppendEvent(
        task.project_id, "task.registered",
        {{"task_id", task.task_id}, {"title", task.title},
         {"assigned_agent", task.assigned_agent}});
    if (!event.ok()) return Expected<Task>(event.status());
    if (guard.has_value()) {
        auto committed = guard->Commit();
        if (!committed.ok()) return Expected<Task>(committed);
    }
    return task;
}

Status Engine::AddDependency(const std::string& task_id, const std::string& depends_on) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return schema;
    if (task_id.empty() || depends_on.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "dependency requires both task ids");
    }
    if (task_id == depends_on) {
        return Status::Error(ErrorCode::InvalidArgument, "a task cannot depend on itself");
    }
    auto task = GetTask(task_id);
    if (!task.ok()) return task.status();
    auto dependency = GetTask(depends_on);
    if (!dependency.ok()) return dependency.status();
    // Reject any edge that would create a cycle: depends_on must not already depend on task_id.
    if (DependsTransitivelyOn(store_, depends_on, task_id)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "dependency would create a cycle: " + task_id + " -> " + depends_on);
    }
    return store_->Exec("INSERT OR IGNORE INTO task_engine_deps(task_id, depends_on) VALUES(?,?);",
                        {storage::SqlValue::Text(task_id), storage::SqlValue::Text(depends_on)});
}

Expected<Task> Engine::GetTask(const std::string& task_id) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<Task>(schema);
    auto rows = store_->Query(
        "SELECT task_id, project_id, title, contract, state, assigned_agent, created_at, "
        "updated_at FROM task_engine_tasks WHERE task_id=?;",
        {storage::SqlValue::Text(task_id)});
    if (!rows.ok()) return Expected<Task>(rows.status());
    if (rows.value().empty()) {
        return Fail<Task>(ErrorCode::NotFound, "task not found: " + task_id);
    }
    return RowToTask(rows.value()[0]);
}

Expected<std::vector<Task>> Engine::TasksFor(const std::string& project_id) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<std::vector<Task>>(schema);
    auto rows = store_->Query(
        "SELECT task_id, project_id, title, contract, state, assigned_agent, created_at, "
        "updated_at FROM task_engine_tasks WHERE project_id=? ORDER BY created_at ASC, task_id "
        "ASC;",
        {storage::SqlValue::Text(project_id)});
    if (!rows.ok()) return Expected<std::vector<Task>>(rows.status());
    std::vector<Task> tasks;
    for (const auto& row : rows.value()) {
        auto task = RowToTask(row);
        if (!task.ok()) return Expected<std::vector<Task>>(task.status());
        tasks.push_back(task.take());
    }
    return tasks;
}

// --- Scheduling ----------------------------------------------------------------------------

Expected<ScheduleDecision> Engine::EvaluateSchedule(const std::string& project_id) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<ScheduleDecision>(schema);
    auto tasks_result = TasksFor(project_id);
    if (!tasks_result.ok()) return Expected<ScheduleDecision>(tasks_result.status());
    std::vector<Task> tasks = tasks_result.take();

    std::set<std::string> completed;
    for (const auto& task : tasks) {
        if (task.state == TaskState::Completed) completed.insert(task.task_id);
    }

    // Write sets already reserved by tasks that are ready, leased or executing.
    struct Reserved {
        std::string task_id;
        std::vector<std::string> writes;
    };
    std::vector<Reserved> reserved;
    for (const auto& task : tasks) {
        if (task.state == TaskState::Ready || task.state == TaskState::Leased ||
            task.state == TaskState::Running || task.state == TaskState::Integrating) {
            reserved.push_back({task.task_id, task.contract.allowed_write_paths});
        }
    }

    ScheduleDecision decision;
    for (const auto& task : tasks) {
        if (task.state != TaskState::Pending) continue;
        // Dependency gate.
        auto deps = store_->Query(
            "SELECT depends_on FROM task_engine_deps WHERE task_id=? ORDER BY depends_on ASC;",
            {storage::SqlValue::Text(task.task_id)});
        if (!deps.ok()) return Expected<ScheduleDecision>(deps.status());
        bool dependency_blocked = false;
        std::string blocking_dependency;
        for (const auto& row : deps.value()) {
            const std::string dependency = row.Text("depends_on");
            if (completed.find(dependency) == completed.end()) {
                dependency_blocked = true;
                blocking_dependency = dependency;
                break;
            }
        }
        if (dependency_blocked) {
            decision.blocked_tasks.push_back(task.task_id);
            decision.reasons[task.task_id] = "dependency not completed: " + blocking_dependency;
            continue;
        }
        // Conflict gate against reserved write sets.
        bool conflict = false;
        std::string conflict_reason;
        for (const auto& active : reserved) {
            if (WriteSetsConflict(task.contract.allowed_write_paths, active.writes)) {
                conflict = true;
                conflict_reason = "write conflict with task " + active.task_id;
                break;
            }
        }
        if (conflict) {
            decision.blocked_tasks.push_back(task.task_id);
            decision.reasons[task.task_id] = conflict_reason;
            continue;
        }
        decision.ready_tasks.push_back(task.task_id);
        reserved.push_back({task.task_id, task.contract.allowed_write_paths});
        // Promote Pending -> Ready (persisted).
        auto promoted = store_->Exec(
            "UPDATE task_engine_tasks SET state=?, updated_at=? WHERE task_id=?;",
            {storage::SqlValue::Text(TaskStateName(TaskState::Ready)),
             storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(task.task_id)});
        if (!promoted.ok()) return Expected<ScheduleDecision>(promoted);
    }
    return decision;
}

// --- Leases --------------------------------------------------------------------------------

Expected<Lease> Engine::GrantLease(const std::string& task_id, const std::string& attempt_id,
                                   const std::string& agent, std::chrono::milliseconds ttl) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<Lease>(schema);
    auto task_result = GetTask(task_id);
    if (!task_result.ok()) return Expected<Lease>(task_result.status());
    Task task = task_result.take();
    // A lease may be (re-)granted to a READY task, or to an already-LEASED task to bump the
    // fencing version and revoke the previous active lease.
    if (task.state != TaskState::Ready && task.state != TaskState::Leased) {
        return Fail<Lease>(ErrorCode::NotReady,
                           "lease can only be granted to a READY task (state=" +
                               std::string(TaskStateName(task.state)) + ")");
    }
    if (attempt_id.empty()) {
        return Fail<Lease>(ErrorCode::InvalidArgument, "lease requires an attempt id");
    }
    if (!agent.empty() && !IsKnownAgent(agent)) {
        return Fail<Lease>(ErrorCode::InvalidArgument, "lease agent is not registered: " + agent);
    }

    auto version_rows = store_->Query(
        "SELECT COALESCE(MAX(version), 0) + 1 AS next FROM task_engine_leases WHERE task_id=?;",
        {storage::SqlValue::Text(task_id)});
    if (!version_rows.ok()) return Expected<Lease>(version_rows.status());
    std::int64_t version = version_rows.value().empty() ? 1 : version_rows.value()[0].Int("next");

    bool own_transaction = !store_->InTransaction();
    std::optional<storage::Transaction> guard;
    if (own_transaction) {
        auto begun = store_->Begin();
        if (!begun.ok()) return Expected<Lease>(begun.status());
        guard.emplace(std::move(begun.value()));
    }
    // Revoke any previous ACTIVE lease for this task (fencing: the new version supersedes it).
    auto revoke = store_->Exec(
        "UPDATE task_engine_leases SET state='REVOKED', revoked_at=? WHERE task_id=? AND "
        "state='ACTIVE';",
        {storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(task_id)});
    if (!revoke.ok()) return Expected<Lease>(revoke);

    Lease lease;
    lease.lease_id = NewId("lease");
    lease.task_id = task_id;
    lease.attempt_id = attempt_id;
    lease.version = version;
    lease.state = "ACTIVE";
    lease.issued_at = NowUtcIso8601();
    const std::int64_t expires_ms = UnixTimeMillis() + static_cast<std::int64_t>(ttl.count());
    lease.expires_at = NowUtcIso8601();
    lease.allowed_reads = task.contract.allowed_read_paths;
    lease.allowed_writes = task.contract.allowed_write_paths;

    auto insert = store_->Exec(
        "INSERT INTO task_engine_leases(lease_id, task_id, attempt_id, version, state, issued_at, "
        "expires_at, expires_at_ms, revoked_at, allowed_reads, allowed_writes) "
        "VALUES(?,?,?,?,?,?,?,?,NULL,?,?);",
        {storage::SqlValue::Text(lease.lease_id), storage::SqlValue::Text(lease.task_id),
         storage::SqlValue::Text(lease.attempt_id), storage::SqlValue::Int(lease.version),
         storage::SqlValue::Text(lease.state), storage::SqlValue::Text(lease.issued_at),
         storage::SqlValue::Text(lease.expires_at), storage::SqlValue::Int(expires_ms),
         storage::SqlValue::Text(StringArrayToJson(lease.allowed_reads).dump()),
         storage::SqlValue::Text(StringArrayToJson(lease.allowed_writes).dump())});
    if (!insert.ok()) return Expected<Lease>(insert);

    auto update = store_->Exec(
        "UPDATE task_engine_tasks SET state=?, assigned_agent=?, updated_at=? WHERE task_id=?;",
        {storage::SqlValue::Text(TaskStateName(TaskState::Leased)),
         storage::SqlValue::Text(agent.empty() ? task.assigned_agent : agent),
         storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(task_id)});
    if (!update.ok()) return Expected<Lease>(update);

    auto event = store_->AppendEvent(
        task.project_id, "task.leased",
        {{"task_id", task_id}, {"lease_id", lease.lease_id}, {"attempt_id", attempt_id},
         {"version", lease.version}});
    if (!event.ok()) return Expected<Lease>(event.status());
    if (guard.has_value()) {
        auto committed = guard->Commit();
        if (!committed.ok()) return Expected<Lease>(committed);
    }
    return lease;
}

Status Engine::ValidateLease(const std::string& lease_id, std::int64_t expected_version) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return schema;
    auto rows = store_->Query(
        "SELECT version, state, expires_at_ms FROM task_engine_leases WHERE lease_id=?;",
        {storage::SqlValue::Text(lease_id)});
    if (!rows.ok()) return rows.status();
    if (rows.value().empty()) {
        return Status::Error(ErrorCode::NotFound, "lease not found: " + lease_id);
    }
    const auto& row = rows.value()[0];
    if (row.Text("state") != "ACTIVE") {
        return Status::Error(ErrorCode::Stale,
                             "lease is not active (state=" + row.Text("state") + ")");
    }
    if (row.Int("version") != expected_version) {
        return Status::Error(ErrorCode::Stale, "lease version mismatch: expected " +
                                                   std::to_string(expected_version) +
                                                   " but lease is at " +
                                                   std::to_string(row.Int("version")));
    }
    const std::int64_t expires_ms = row.Int("expires_at_ms");
    if (expires_ms > 0 && UnixTimeMillis() >= expires_ms) {
        return Status::Error(ErrorCode::Stale, "lease has expired");
    }
    return Status::Ok();
}

Status Engine::RevokeLease(const std::string& lease_id, std::int64_t expected_version) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return schema;
    auto rows = store_->Query("SELECT version, state FROM task_engine_leases WHERE lease_id=?;",
                              {storage::SqlValue::Text(lease_id)});
    if (!rows.ok()) return rows.status();
    if (rows.value().empty()) {
        return Status::Error(ErrorCode::NotFound, "lease not found: " + lease_id);
    }
    const auto& row = rows.value()[0];
    if (row.Int("version") != expected_version) {
        return Status::Error(ErrorCode::Conflict, "lease version moved: expected " +
                                                      std::to_string(expected_version) +
                                                      " but lease is at " +
                                                      std::to_string(row.Int("version")));
    }
    if (row.Text("state") != "ACTIVE") {
        return Status::Error(ErrorCode::Conflict,
                             "lease is not active (state=" + row.Text("state") + ")");
    }
    return store_->Exec(
        "UPDATE task_engine_leases SET state='REVOKED', revoked_at=? WHERE lease_id=?;",
        {storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(lease_id)});
}

Expected<Lease> Engine::ActiveLeaseFor(const std::string& task_id) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<Lease>(schema);
    auto rows = store_->Query(
        "SELECT lease_id, task_id, attempt_id, version, state, issued_at, expires_at, "
        "allowed_reads, allowed_writes FROM task_engine_leases WHERE task_id=? AND state='ACTIVE' "
        "ORDER BY version DESC LIMIT 1;",
        {storage::SqlValue::Text(task_id)});
    if (!rows.ok()) return Expected<Lease>(rows.status());
    if (rows.value().empty()) {
        return Fail<Lease>(ErrorCode::NotFound, "no active lease for task: " + task_id);
    }
    const auto& row = rows.value()[0];
    Lease lease;
    lease.lease_id = row.Text("lease_id");
    lease.task_id = row.Text("task_id");
    lease.attempt_id = row.Text("attempt_id");
    lease.version = row.Int("version");
    lease.state = row.Text("state");
    lease.issued_at = row.Text("issued_at");
    lease.expires_at = row.Text("expires_at");
    auto reads = ParseJsonBounded(row.Text("allowed_reads"));
    if (reads.ok() && reads.value().is_array()) {
        lease.allowed_reads = reads.value().get<std::vector<std::string>>();
    }
    auto writes = ParseJsonBounded(row.Text("allowed_writes"));
    if (writes.ok() && writes.value().is_array()) {
        lease.allowed_writes = writes.value().get<std::vector<std::string>>();
    }
    return lease;
}

// --- Attempts ------------------------------------------------------------------------------

Expected<Attempt> Engine::StartAttempt(const std::string& task_id, const std::string& agent,
                                       std::int64_t lease_version,
                                       const nlohmann::json& baseline) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<Attempt>(schema);
    auto task_result = GetTask(task_id);
    if (!task_result.ok()) return Expected<Attempt>(task_result.status());
    Task task = task_result.take();
    if (task.state != TaskState::Leased && task.state != TaskState::Running) {
        return Fail<Attempt>(ErrorCode::NotReady,
                             "attempt requires a leased task (state=" +
                                 std::string(TaskStateName(task.state)) + ")");
    }
    auto lease = ActiveLeaseFor(task_id);
    if (!lease.ok()) return Fail<Attempt>(ErrorCode::NotReady, "no active lease for task: " + task_id);
    if (lease.value().version != lease_version) {
        return Fail<Attempt>(ErrorCode::Stale, "lease version mismatch: attempt requested " +
                                                   std::to_string(lease_version) +
                                                   " but active lease is at " +
                                                   std::to_string(lease.value().version));
    }

    Attempt attempt;
    attempt.attempt_id = NewId("att");
    attempt.task_id = task_id;
    attempt.agent = agent.empty() ? task.assigned_agent : agent;
    attempt.status = "RUNNING";
    attempt.lease_version = lease_version;
    attempt.session_id = "";
    attempt.baseline = baseline;
    attempt.result = nlohmann::json::object();
    attempt.started_at = NowUtcIso8601();

    bool own_transaction = !store_->InTransaction();
    std::optional<storage::Transaction> guard;
    if (own_transaction) {
        auto begun = store_->Begin();
        if (!begun.ok()) return Expected<Attempt>(begun.status());
        guard.emplace(std::move(begun.value()));
    }
    auto insert = store_->Exec(
        "INSERT INTO task_engine_attempts(attempt_id, task_id, agent, status, lease_version, "
        "session_id, baseline, result, started_at, ended_at) VALUES(?,?,?,?,?,?,?,?,?,NULL);",
        {storage::SqlValue::Text(attempt.attempt_id), storage::SqlValue::Text(attempt.task_id),
         storage::SqlValue::Text(attempt.agent), storage::SqlValue::Text(attempt.status),
         storage::SqlValue::Int(attempt.lease_version), storage::SqlValue::Text(attempt.session_id),
         storage::SqlValue::Text(attempt.baseline.dump()),
         storage::SqlValue::Text(attempt.result.dump()),
         storage::SqlValue::Text(attempt.started_at)});
    if (!insert.ok()) return Expected<Attempt>(insert);
    auto update = store_->Exec(
        "UPDATE task_engine_tasks SET state=?, updated_at=? WHERE task_id=? AND state=?;",
        {storage::SqlValue::Text(TaskStateName(TaskState::Running)),
         storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(task_id),
         storage::SqlValue::Text(TaskStateName(TaskState::Leased))});
    if (!update.ok()) return Expected<Attempt>(update);
    if (guard.has_value()) {
        auto committed = guard->Commit();
        if (!committed.ok()) return Expected<Attempt>(committed);
    }
    return attempt;
}

Status Engine::FinishAttempt(const std::string& attempt_id, const std::string& status,
                             const nlohmann::json& result) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return schema;
    if (!IsKnownAttemptStatus(status)) {
        return Status::Error(ErrorCode::InvalidArgument, "unknown attempt status: " + status);
    }
    auto rows = store_->Query(
        "SELECT task_id, status FROM task_engine_attempts WHERE attempt_id=?;",
        {storage::SqlValue::Text(attempt_id)});
    if (!rows.ok()) return rows.status();
    if (rows.value().empty()) {
        return Status::Error(ErrorCode::NotFound, "attempt not found: " + attempt_id);
    }
    const std::string task_id = rows.value()[0].Text("task_id");
    auto update = store_->Exec(
        "UPDATE task_engine_attempts SET status=?, result=?, ended_at=? WHERE attempt_id=?;",
        {storage::SqlValue::Text(status), storage::SqlValue::Text(result.dump()),
         storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(attempt_id)});
    if (!update.ok()) return update;
    auto task = GetTask(task_id);
    if (task.ok()) {
        auto event = store_->AppendEvent(
            task.value().project_id, status == "FAILED" ? "attempt.failed" : "attempt.finished",
            {{"attempt_id", attempt_id}, {"task_id", task_id}, {"status", status}});
        if (!event.ok()) return event.status();
    }
    return Status::Ok();
}

Expected<AttemptEvidenceBundle> Engine::RecordAttemptEvidence(
    const AttemptEvidenceBundle& bundle) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<AttemptEvidenceBundle>(schema);
    if (bundle.task_id.empty() || bundle.attempt_id.empty()) {
        return Fail<AttemptEvidenceBundle>(ErrorCode::InvalidArgument,
                                           "evidence bundle requires task_id and attempt_id");
    }
    AttemptEvidenceBundle stored = bundle;
    if (stored.bundle_id.empty()) stored.bundle_id = NewId("bundle");
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();
    auto exists = store_->Query(
        "SELECT bundle_id FROM task_engine_attempt_evidence WHERE bundle_id=?;",
        {storage::SqlValue::Text(stored.bundle_id)});
    if (!exists.ok()) return Expected<AttemptEvidenceBundle>(exists.status());
    if (!exists.value().empty()) {
        return Fail<AttemptEvidenceBundle>(ErrorCode::AlreadyExists,
                                           "evidence bundle already recorded: " + stored.bundle_id);
    }
    auto insert = store_->Exec(
        "INSERT INTO task_engine_attempt_evidence(bundle_id, task_id, attempt_id, lease_id, "
        "lease_version, agent, session_id, baseline, observed_changes, process_outcomes, "
        "diagnostics, proposed_criterion_evidence, known_omissions, created_at) "
        "VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(stored.bundle_id), storage::SqlValue::Text(stored.task_id),
         storage::SqlValue::Text(stored.attempt_id), storage::SqlValue::Text(stored.lease_id),
         storage::SqlValue::Int(stored.lease_version), storage::SqlValue::Text(stored.agent),
         storage::SqlValue::Text(stored.session_id),
         storage::SqlValue::Text(stored.baseline.dump()),
         storage::SqlValue::Text(nlohmann::json(stored.observed_changes).dump()),
         storage::SqlValue::Text(stored.process_outcomes.dump()),
         storage::SqlValue::Text(StringArrayToJson(stored.diagnostics).dump()),
         storage::SqlValue::Text(nlohmann::json(stored.proposed_criterion_evidence).dump()),
         storage::SqlValue::Text(StringArrayToJson(stored.known_omissions).dump()),
         storage::SqlValue::Text(stored.created_at)});
    if (!insert.ok()) return Expected<AttemptEvidenceBundle>(insert);
    auto task = GetTask(stored.task_id);
    if (task.ok()) {
        auto event = store_->AppendEvent(
            task.value().project_id, "evidence.recorded",
            {{"bundle_id", stored.bundle_id}, {"task_id", stored.task_id},
             {"attempt_id", stored.attempt_id}, {"lease_version", stored.lease_version}});
        if (!event.ok()) return Expected<AttemptEvidenceBundle>(event.status());
    }
    return stored;
}

Expected<std::vector<Attempt>> Engine::AttemptsFor(const std::string& task_id) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<std::vector<Attempt>>(schema);
    auto rows = store_->Query(
        "SELECT attempt_id, task_id, agent, status, lease_version, session_id, baseline, result, "
        "started_at, ended_at FROM task_engine_attempts WHERE task_id=? ORDER BY started_at ASC, "
        "attempt_id ASC;",
        {storage::SqlValue::Text(task_id)});
    if (!rows.ok()) return Expected<std::vector<Attempt>>(rows.status());
    std::vector<Attempt> attempts;
    for (const auto& row : rows.value()) {
        Attempt attempt;
        attempt.attempt_id = row.Text("attempt_id");
        attempt.task_id = row.Text("task_id");
        attempt.agent = row.Text("agent");
        attempt.status = row.Text("status");
        attempt.lease_version = row.Int("lease_version");
        attempt.session_id = row.Text("session_id");
        attempt.started_at = row.Text("started_at");
        attempt.ended_at = row.Text("ended_at");
        auto baseline = ParseJsonBounded(row.Text("baseline"));
        if (baseline.ok()) attempt.baseline = baseline.value();
        auto result = ParseJsonBounded(row.Text("result"));
        if (result.ok()) attempt.result = result.value();
        attempts.push_back(std::move(attempt));
    }
    return attempts;
}

// --- State transitions ---------------------------------------------------------------------

Status Engine::TransitionTask(const std::string& task_id, TaskState state) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return schema;
    auto task_result = GetTask(task_id);
    if (!task_result.ok()) return task_result.status();
    Task task = task_result.take();
    if (!IsLegalTransition(task.state, state)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "illegal task transition: " + std::string(TaskStateName(task.state)) +
                                 " -> " + std::string(TaskStateName(state)));
    }
    auto update = store_->Exec(
        "UPDATE task_engine_tasks SET state=?, updated_at=? WHERE task_id=?;",
        {storage::SqlValue::Text(TaskStateName(state)), storage::SqlValue::Text(NowUtcIso8601()),
         storage::SqlValue::Text(task_id)});
    if (!update.ok()) return update;
    auto event = store_->AppendEvent(
        task.project_id, "task.state.changed",
        {{"task_id", task_id}, {"from", TaskStateName(task.state)}, {"to", TaskStateName(state)}});
    if (!event.ok()) return event.status();
    return Status::Ok();
}

// --- Lease expiry --------------------------------------------------------------------------

Expected<std::vector<std::string>> Engine::ExpireStaleLeases(const std::string& project_id) {
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<std::vector<std::string>>(schema);
    const std::int64_t now = UnixTimeMillis();
    auto rows = store_->Query(
        "SELECT l.lease_id, l.task_id, l.version FROM task_engine_leases l JOIN "
        "task_engine_tasks t ON t.task_id = l.task_id WHERE t.project_id=? AND l.state='ACTIVE' "
        "AND l.expires_at_ms > 0 AND l.expires_at_ms <= ?;",
        {storage::SqlValue::Text(project_id), storage::SqlValue::Int(now)});
    if (!rows.ok()) return Expected<std::vector<std::string>>(rows.status());
    if (rows.value().empty()) return std::vector<std::string>{};

    bool own_transaction = !store_->InTransaction();
    std::optional<storage::Transaction> guard;
    if (own_transaction) {
        auto begun = store_->Begin();
        if (!begun.ok()) return Expected<std::vector<std::string>>(begun.status());
        guard.emplace(std::move(begun.value()));
    }
    std::vector<std::string> expired_tasks;
    for (const auto& row : rows.value()) {
        const std::string lease_id = row.Text("lease_id");
        const std::string task_id = row.Text("task_id");
        const std::int64_t version = row.Int("version");
        auto revoke = store_->Exec(
            "UPDATE task_engine_leases SET state='EXPIRED', revoked_at=? WHERE lease_id=?;",
            {storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(lease_id)});
        if (!revoke.ok()) return Expected<std::vector<std::string>>(revoke);
        // Return the task to READY (from a lease-held state).
        auto task = store_->Exec(
            "UPDATE task_engine_tasks SET state=?, updated_at=? WHERE task_id=? AND state IN "
            "('leased','running','integrating');",
            {storage::SqlValue::Text(TaskStateName(TaskState::Ready)),
             storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(task_id)});
        if (!task.ok()) return Expected<std::vector<std::string>>(task);
        // Never silently consume a retry: an interrupted attempt is UNKNOWN, not SUCCEEDED.
        auto attempts = store_->Exec(
            "UPDATE task_engine_attempts SET status='UNKNOWN', ended_at=? WHERE task_id=? AND "
            "lease_version=? AND status IN ('STARTED','RUNNING');",
            {storage::SqlValue::Text(NowUtcIso8601()), storage::SqlValue::Text(task_id),
             storage::SqlValue::Int(version)});
        if (!attempts.ok()) return Expected<std::vector<std::string>>(attempts);
        auto event = store_->AppendEvent(
            project_id, "lease.expired",
            {{"task_id", task_id}, {"lease_id", lease_id}, {"version", version}});
        if (!event.ok()) return Expected<std::vector<std::string>>(event.status());
        expired_tasks.push_back(task_id);
    }
    if (guard.has_value()) {
        auto committed = guard->Commit();
        if (!committed.ok()) return Expected<std::vector<std::string>>(committed);
    }
    return expired_tasks;
}

}  // namespace mayasaba::tasks
