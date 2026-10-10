// Policy Engine (layer 12): authorization of every material action, with recorded denials.
//
// Default-deny: an action is admitted only when the effective grants (task contract, workspace
// view, user authorization, assignment scope) cover it. Prompt wording, agent statements,
// acknowledgements and factual observations never grant authority.
#pragma once

#include <cstdint>
#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::policy {

enum class ActionKind {
    ReadPath,            // read a file under the project root / workspace view
    WritePath,           // write a file inside an authorized isolated workspace
    PublishFile,         // controller publication into the user's canonical root
    LaunchProcess,       // controller-issued process launch
    AgentSession,        // start a CLI session for a task/assignment
    NativeWebTool,       // assignment-scoped CLI-native read-only web search/fetch
    MaterialAction,      // any other material side effect requiring an operation identity
};

const char* ActionKindName(ActionKind kind);

struct ActionRequest {
    ActionKind kind = ActionKind::ReadPath;
    std::string project_id;
    std::string task_id;            // empty for non-task actions
    std::string attempt_id;
    std::string lease_id;
    std::int64_t lease_version = 0;
    std::string path;               // canonical absolute path where applicable
    std::string operation_id;       // material actions: idempotency identity
    nlohmann::json subject = nlohmann::json::object();
};

struct AuthorizationDecision {
    bool allowed = false;
    std::string reason;             // exact deny reason, recorded
    std::string policy_ref;         // which rule/authority granted or denied
};

struct GrantSet {
    // Authorized scopes for a subject (task/assignment). Paths are canonical absolute paths.
    std::vector<std::string> readable_roots;      // e.g. the project root or a read-only view
    std::vector<std::string> writable_roots;      // isolated workspace only
    std::vector<std::string> publish_roots;       // canonical project root for publication
    bool allow_process_launch = false;
    bool allow_agent_session = false;
    bool allow_native_web_tools = false;
    std::string authority_ref;                    // user authorization / requirement / decision
};

class Engine {
public:
    explicit Engine(storage::Store* store) : store_(store) {}

    // Registers the effective grant set for a task/attempt or a named subject key.
    Status Grant(const std::string& subject_key, const GrantSet& grants);
    Status Revoke(const std::string& subject_key);
    std::optional<GrantSet> GrantsFor(const std::string& subject_key);

    // Authorizes (or denies) one material action. Denials are persisted with their reason.
    AuthorizationDecision Authorize(const ActionRequest& request);

    // Records an explicit denial (used when a service refuses without an action request).
    Status RecordDenial(const std::string& project_id, ActionKind kind,
                        const nlohmann::json& subject, const std::string& reason);

    // Convenience: require authorization, returning a DENIED status on refusal.
    Status Require(const ActionRequest& request);

    Expected<std::vector<nlohmann::json>> DenialsFor(const std::string& project_id);

private:
    storage::Store* store_;
    std::map<std::string, GrantSet> grants_;
    std::mutex mutex_;
};

}  // namespace mayasaba::policy
