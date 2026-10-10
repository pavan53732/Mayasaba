// Controller Application Services (layer 2): the Orchestrator and facade that own
// cross-service scheduling, barriers, registered decision/trigger points and phase transitions.
// It does NOT mutate the project epoch or another service's records. UI and agent text are not
// authority; authorization, approved requirements, binding decisions and valid task/lease
// commands define what the controller may do.
#pragma once

#include <atomic>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/adapter.hpp"
#include "mayasaba/base.hpp"
#include "mayasaba/council.hpp"
#include "mayasaba/context.hpp"
#include "mayasaba/evidence.hpp"
#include "mayasaba/gateway.hpp"
#include "mayasaba/policy.hpp"
#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/tasks.hpp"
#include "mayasaba/validation.hpp"
#include "mayasaba/workspace.hpp"

namespace mayasaba::app {

// Registered decision trigger: the Orchestrator owns the trigger; the Council produces the
// recommendation and the Decision service binds the outcome. Silence, ACK, majority or agent
// claims are never approval.
enum class TriggerKind { ExplorationComplete, CouncilPointResolved, LeaseExpired,
                         ValidationPassed, UserMilestone };
const char* TriggerKindName(TriggerKind kind);

struct RegisteredTrigger {
    std::string trigger_id;
    TriggerKind kind;
    std::string project_id;
    std::string target_task_id;          // may be empty for project-level triggers
    std::string council_point_id;        // linked when kind == CouncilPointResolved
    bool fired = false;
    std::string created_at;
    std::string fired_at;
};

// A user contribution to the Chat thread: immutable, one per Send after a root is bound.
struct ChatContribution {
    std::string contribution_id;
    std::string project_id;
    std::string text;
    std::vector<std::string> attachments;       // evidence IDs / content references
    std::string created_at;
};

enum class ProjectPhase { Unbound, Bound, Exploration, Execution, Integrating,
                         Validation, Completed, Paused, Closed, Abandoned };
const char* ProjectPhaseName(ProjectPhase phase);
std::optional<ProjectPhase> ParseProjectPhase(const std::string& name);

struct ProjectIdentity {
    std::string project_id;
    std::string canonical_root;
    std::string display_path;
    std::int64_t epoch = 0;
    std::string created_at;
};

struct SendResult {
    std::string contribution_id;
    std::string outcome;               // PERSISTED | DENIED | BLOCKED
    std::string detail;
};

class Orchestrator {
public:
    struct Services {
        storage::Store* store = nullptr;
        policy::Engine* policy = nullptr;
        tasks::Engine* tasks = nullptr;
        council::Engine* council = nullptr;
        context::Synchronizer* context = nullptr;
        evidence::Engine* evidence = nullptr;
        validation::Engine* validation = nullptr;
        gateway::AgentGateway* gateway = nullptr;
    };
    explicit Orchestrator(Services services) : services_(services) {}

    // --- Project lifecycle (project service owns identity/epoch) -------------------------
    Expected<ProjectIdentity> BindProjectRoot(const std::string& display_path,
                                              const std::string& canonical_root = {});
    Expected<ProjectIdentity> Project(const std::string& project_id);

    // --- Chat (the only persistent user-facing workspace) --------------------------------
    // Every Send after binding persists exactly one immutable UserContribution, even if no CLI
    // is available. Send is disabled only until an authorized project root is bound.
    Expected<SendResult> SubmitUserContribution(const std::string& project_id,
                                                const std::string& text,
                                                const std::vector<std::string>& attachments = {});
    Expected<ChatContribution> Contribution(const std::string& contribution_id);

    // --- Authorization gate for every material action ------------------------------------
    Status RequireAuthorization(const policy::ActionRequest& request) {
        return services_.policy->Require(request);
    }

    // --- Task execution orchestration ----------------------------------------------------
    Expected<tasks::Task> ScheduleTask(const std::string& project_id, const std::string& title,
                                       const tasks::TaskContract& contract);
    Expected<tasks::Lease> LeaseTask(const std::string& task_id, const std::string& agent);
    // Starts the leased agent session for a task in its isolated staging and records the
    // attempt. Returns the controller-owned session id. All process launch goes through the
    // gateway/kernel stack; this never spawns a process directly.
    Expected<std::string> StartTaskExecution(const std::string& task_id,
                                             const std::string& attempt_id,
                                             const std::string& agent);

    // Completion routing: the application pump reports a finished agent session here; the
    // orchestrator advances the task/council state machines it coordinates. It never mutates
    // another service's records directly — it issues registered commands to those engines.
    Status OnAgentSessionCompleted(const std::string& session_id, bool success,
                                   const std::string& session_error);

    // Integration pipeline (after a successful attempt): change set, conflict check,
    // guarded publication through the durable journal.
    Expected<workspace::IntegrationCandidate> PrepareIntegration(const std::string& task_id);
    Expected<workspace::PublicationOutcome> PublishIntegration(const std::string& task_id,
                                                               const std::string& recovery_root = {});

    // --- Validation and task completion --------------------------------------------------
    // Evaluates task acceptance criteria against accumulated evidence via validation engine.
    // Invariant (AGENTS.md §§ 4.4, 13): Unknown or unverifiable state fails closed.
    // If criteria are empty or no validation verdicts are produced, or if any verdict is not
    // Pass/NotApplicable, the task transitions to Failed. Only when !verdicts.empty() and every
    // verdict is Pass or NotApplicable does the task transition to Completed.
    Expected<std::vector<validation::ValidationResult>> ValidateTask(const std::string& task_id);

    // --- Full council orchestration (FULL is the only mode) ------------------------------
    Expected<std::string> OpenCouncilPoint(const std::string& project_id,
                                           const std::string& topic,
                                           const std::string& evidence,
                                           const std::string& snapshot_id);
    Status AdvanceCouncilRound(const std::string& point_id);

    // Council session links: which agent session carries which point/round, so the
    // application pump can route completed proposals/critiques back into the council engine.
    struct CouncilSessionLink {
        std::string point_id;
        std::int64_t round = 0;
        std::string agent;
        std::string session_id;
        std::string role;                 // "proposal" | "critique" | "synthesis"
    };
    std::vector<CouncilSessionLink> CouncilSessions();
    Status LinkCouncilSession(const CouncilSessionLink& link);
    Status SubmitCouncilProposal(const std::string& point_id, const std::string& agent,
                                 const std::string& proposal_text,
                                 const nlohmann::json& evidence = nlohmann::json::array());
    Status SubmitCouncilCritique(const std::string& point_id, const std::string& author,
                                 const std::string& target, const std::string& review);
    Status SubmitCouncilSynthesis(const std::string& point_id, const std::string& chair,
                                  const std::string& text, const std::string& nonchair_review,
                                  const std::vector<nlohmann::json>& disagreements = {});
    Expected<council::Engine::PointState> CouncilPointState(const std::string& point_id);

    // --- Registered triggers (controller-owned scheduling authority) ---------------------
    Status RegisterTrigger(const RegisteredTrigger& trigger);
    Status FireTrigger(const std::string& trigger_id);
    std::vector<RegisteredTrigger> Triggers(const std::string& project_id);

    // --- Phase transitions ---------------------------------------------------------------
    Status TransitionPhase(const std::string& project_id, ProjectPhase phase);

private:
    Services services_;
    std::mutex mutex_;
    std::vector<CouncilSessionLink> council_sessions_;
    std::map<std::string, workspace::StagedWorkspace> staging_;   // task_id -> staged workspace

    // session_id -> task/attempt link for completion routing (coordination state, not an
    // authoritative record: the attempt/lease records live in the Task engine's tables).
    struct TaskSessionLink {
        std::string task_id;
        std::string attempt_id;
        std::int64_t lease_version = 0;
        std::string staging_path;
    };
    std::map<std::string, TaskSessionLink> task_sessions_;

    Status ValidatePhaseTransition(ProjectPhase from, ProjectPhase to);
    Expected<ProjectIdentity> LoadProject(const std::string& project_id);
    Expected<workspace::StagedWorkspace> EnsureStaging(const tasks::Task& task,
                                                       const ProjectIdentity& project);
    Status GrantTaskScopes(const tasks::Task& task, const ProjectIdentity& project,
                           const std::string& writable_root);
    std::string WorkspaceRoot() const;
    std::string BuildTaskPrompt(const tasks::Task& task) const;
    Status FireTriggerLocked(const std::string& trigger_id);
    std::vector<RegisteredTrigger> TriggersLocked(const std::string& project_id);
};

}  // namespace mayasaba::app
