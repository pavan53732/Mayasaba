// Task/DAG Engine (layer 7): task graph, contracts, attempts, leases with fencing,
// dependency- and conflict-aware scheduling, and attempt evidence bundles.
//
// A task is the stable unit of acceptance; an attempt is one execution; the active lease
// version is the single fencing token for lease-derived material effects. A lease record is
// not a filesystem sandbox: enforcement comes from the policy/workspace boundaries.
#pragma once

#include <chrono>
#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::tasks {

struct AcceptanceCriterion {
    std::string criterion_id;
    std::string requirement_id;
    std::string expectation;         // observable expectation
    std::string oracle_class;        // policy | artifact_integrity | static | compile | unit |
                                     // integration | runtime | e2e | data_integrity | citation |
                                     // user_acceptance
    std::string oracle_spec;         // check command / procedure description (when applicable)
    bool blocking = true;
    std::string evidence_scope;      // artifact path/hash scope
};

struct TaskContract {
    std::string task_id;
    std::string project_id;
    std::string work_request_id;
    std::string contribution_id;
    std::string requirement_id;      // owning requirement where applicable
    std::string decision_id;         // owning decision where applicable
    std::int64_t epoch = 0;
    std::string snapshot_id;
    std::string context_digest;
    std::string objective;
    std::vector<std::string> expected_artifacts;
    std::vector<std::string> dependencies;         // task ids
    std::string assigned_agent;                    // "hermes" | "kilo" | "claude" ("" = unassigned)
    std::vector<std::string> allowed_read_paths;   // relative to project root
    std::vector<std::string> allowed_write_paths;  // relative; must stay in staging
    std::vector<std::string> forbidden_paths;
    std::vector<std::string> approved_capabilities;  // e.g. "native-web-search"
    std::int64_t time_budget_ms = 0;
    std::int64_t token_budget = 0;                 // 0 = unavailable/unset, never invented
    std::vector<std::string> expected_change_types;
    std::vector<AcceptanceCriterion> criteria;

    nlohmann::json ToJson() const;
    static Expected<TaskContract> FromJson(const nlohmann::json& value);
};

enum class TaskState {
    Pending, Ready, Leased, Running, Integrating, Validating, Completed, Blocked, Failed,
    Cancelled,
};
const char* TaskStateName(TaskState state);
std::optional<TaskState> ParseTaskState(const std::string& name);

struct Task {
    std::string task_id;
    std::string project_id;
    std::string title;
    TaskContract contract;
    TaskState state = TaskState::Pending;
    std::string assigned_agent;
    std::string created_at;
    std::string updated_at;
};

struct Lease {
    std::string lease_id;
    std::string task_id;
    std::string attempt_id;
    std::int64_t version = 1;
    std::string state;               // ACTIVE | EXPIRED | REVOKED | RELEASED
    std::string issued_at;
    std::string expires_at;
    std::vector<std::string> allowed_reads;
    std::vector<std::string> allowed_writes;
};

struct Attempt {
    std::string attempt_id;
    std::string task_id;
    std::string agent;
    std::string status;              // STARTED | RUNNING | SUCCEEDED | FAILED | CANCELLED | UNKNOWN
    std::int64_t lease_version = 0;
    std::string session_id;
    nlohmann::json baseline = nlohmann::json::object();
    nlohmann::json result = nlohmann::json::object();
    std::string started_at;
    std::string ended_at;
};

// Immutable proof-carrying output of one attempt.
struct AttemptEvidenceBundle {
    std::string bundle_id;
    std::string task_id;
    std::string attempt_id;
    std::string lease_id;
    std::int64_t lease_version = 0;
    std::string agent;
    std::string session_id;
    nlohmann::json baseline;
    std::vector<nlohmann::json> observed_changes;   // path, operation, before/after hashes
    nlohmann::json process_outcomes = nlohmann::json::object();
    std::vector<std::string> diagnostics;
    std::vector<nlohmann::json> proposed_criterion_evidence;
    std::vector<std::string> known_omissions;
    std::string created_at;
};

struct ScheduleDecision {
    std::vector<std::string> ready_tasks;      // dependencies satisfied, no write conflicts
    std::vector<std::string> blocked_tasks;    // with reasons
    std::map<std::string, std::string> reasons;
};

class Engine {
public:
    explicit Engine(storage::Store* store) : store_(store) {}

    // Registers a task from a validated contract. Contract validation is mandatory: absent or
    // unverifiable acceptance oracles and missing identity fields are rejected.
    Expected<Task> RegisterTask(const std::string& title, const TaskContract& contract);

    Status AddDependency(const std::string& task_id, const std::string& depends_on);

    Expected<Task> GetTask(const std::string& task_id);
    Expected<std::vector<Task>> TasksFor(const std::string& project_id);

    // Dependency- and conflict-aware scheduling: a task is READY only when every dependency is
    // COMPLETED and its write set does not overlap an active/leased task's write set
    // (unknown/aliased/generated paths are conservatively treated as conflicting).
    Expected<ScheduleDecision> EvaluateSchedule(const std::string& project_id);

    // Grants a lease for a ready task to an attempt; increments the fencing version and
    // revokes any previous active lease for that task.
    Expected<Lease> GrantLease(const std::string& task_id, const std::string& attempt_id,
                               const std::string& agent,
                               std::chrono::milliseconds ttl = std::chrono::hours(2));

    // Fencing check: validates lease identity and version before a material side effect.
    // Returns STALE when the lease is revoked/expired or the version no longer matches.
    Status ValidateLease(const std::string& lease_id, std::int64_t expected_version);

    // Compare-and-swap lease revocation; returns CONFLICT when the version moved.
    Status RevokeLease(const std::string& lease_id, std::int64_t expected_version);

    Expected<Lease> ActiveLeaseFor(const std::string& task_id);

    // Attempts.
    Expected<Attempt> StartAttempt(const std::string& task_id, const std::string& agent,
                                   std::int64_t lease_version, const nlohmann::json& baseline);
    Status FinishAttempt(const std::string& attempt_id, const std::string& status,
                         const nlohmann::json& result);
    Expected<AttemptEvidenceBundle> RecordAttemptEvidence(const AttemptEvidenceBundle& bundle);
    Expected<std::vector<Attempt>> AttemptsFor(const std::string& task_id);

    // State transitions (registered commands).
    Status TransitionTask(const std::string& task_id, TaskState state);

    // Detects tasks whose leases expired (wall clock) and returns them to READY with the
    // expired lease revoked. Never silently consumes a retry: the attempt is marked UNKNOWN.
    Expected<std::vector<std::string>> ExpireStaleLeases(const std::string& project_id);

private:
    storage::Store* store_;
};

// Contract validation, exposed for tests and the contract tool.
Status ValidateContract(const TaskContract& contract);

}  // namespace mayasaba::tasks
