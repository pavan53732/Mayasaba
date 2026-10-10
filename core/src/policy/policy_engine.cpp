// Policy Engine (layer 12): material-action authorization with recorded, persisted denials.
//
// Default-deny is the core invariant: an action is admitted only when an effective grant set
// (task contract / workspace view / user authorization / assignment scope) covers it. Prompt
// wording, agent statements, acknowledgements and factual observations never grant authority.
//
// Consolidation note (for the parent): the tables this service uses (policy_denials) already
// exist in the v1 storage migration. EnsureSchema() re-asserts them with
// `CREATE TABLE IF NOT EXISTS` lazily as a defensive measure; it introduces no schema change
// and no new owner. Every public entry point that touches SQL calls it.
#include "mayasaba/policy.hpp"

#include <algorithm>
#include <mutex>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::policy {
namespace {

using storage::SqlValue;

constexpr const char* kRefDefaultDeny = "policy.default-deny";
constexpr const char* kRefGrant = "policy.grant";

void EnsureSchema(storage::Store* store) {
    if (!store) return;
    static std::once_flag once;
    std::call_once(once, [store]() {
        (void)store->Exec(
            "CREATE TABLE IF NOT EXISTS policy_denials("
            "denial_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, action TEXT NOT NULL, "
            "subject TEXT NOT NULL DEFAULT '{}', reason TEXT NOT NULL, created_at TEXT NOT NULL);");
    });
}

// True when any path component is exactly "..". Checked on the raw request path before any
// canonicalization so that an escape attempt is refused even if a root prefix looks similar.
bool HasDotDotComponent(const std::string& path) {
    std::size_t i = 0;
    for (;;) {
        std::size_t j = i;
        while (j < path.size() && path[j] != '\\' && path[j] != '/') ++j;
        if (j - i == 2 && path[i] == '.' && path[i + 1] == '.') return true;
        if (j >= path.size()) break;
        i = j + 1;
    }
    return false;
}

std::string SubjectKey(const ActionRequest& request) {
    if (request.subject.is_object() && request.subject.contains("subject_key") &&
        request.subject["subject_key"].is_string()) {
        return request.subject["subject_key"].get<std::string>();
    }
    if (!request.task_id.empty()) return request.task_id;
    return request.project_id;
}

// Authorizes `path` against canonical `roots`. Rejects '..' escapes, non-canonicalizable
// paths and any path not component-wise within a root (so C:\proj never covers C:\proj-evil).
bool PathWithinAny(const std::string& path, const std::vector<std::string>& roots,
                   std::string* reason) {
    if (path.empty()) {
        *reason = "action path is empty";
        return false;
    }
    if (HasDotDotComponent(path)) {
        *reason = "action path contains a '..' component";
        return false;
    }
    auto canonical = fs::CanonicalizePath(path);
    if (!canonical.ok()) {
        *reason = "action path cannot be canonicalized: " + canonical.message();
        return false;
    }
    for (const auto& root : roots) {
        std::string resolved_root = root;
        auto canonical_root = fs::CanonicalizePath(root);
        if (canonical_root.ok()) resolved_root = canonical_root.value();
        if (!resolved_root.empty() && fs::IsPathWithin(resolved_root, canonical.value())) {
            return true;
        }
    }
    *reason = "path '" + canonical.value() + "' is outside the authorized roots";
    return false;
}

}  // namespace

const char* ActionKindName(ActionKind kind) {
    switch (kind) {
        case ActionKind::ReadPath: return "ReadPath";
        case ActionKind::WritePath: return "WritePath";
        case ActionKind::PublishFile: return "PublishFile";
        case ActionKind::LaunchProcess: return "LaunchProcess";
        case ActionKind::AgentSession: return "AgentSession";
        case ActionKind::NativeWebTool: return "NativeWebTool";
        case ActionKind::MaterialAction: return "MaterialAction";
    }
    return "Unknown";
}

Status Engine::Grant(const std::string& subject_key, const GrantSet& grants) {
    if (subject_key.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "grant subject key is empty");
    }
    EnsureSchema(store_);
    std::lock_guard<std::mutex> lock(mutex_);
    grants_[subject_key] = grants;
    return Status::Ok();
}

Status Engine::Revoke(const std::string& subject_key) {
    if (subject_key.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "revoke subject key is empty");
    }
    std::lock_guard<std::mutex> lock(mutex_);
    grants_.erase(subject_key);
    return Status::Ok();
}

std::optional<GrantSet> Engine::GrantsFor(const std::string& subject_key) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = grants_.find(subject_key);
    if (it == grants_.end()) return std::nullopt;
    return it->second;
}

AuthorizationDecision Engine::Authorize(const ActionRequest& request) {
    EnsureSchema(store_);
    AuthorizationDecision decision;

    const std::string subject_key = SubjectKey(request);
    if (subject_key.empty()) {
        decision.allowed = false;
        decision.reason = "no subject key: task and project identity are both empty";
        decision.policy_ref = kRefDefaultDeny;
        (void)RecordDenial(request.project_id, request.kind, request.subject, decision.reason);
        return decision;
    }

    GrantSet grants;
    bool has_grant = false;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = grants_.find(subject_key);
        if (it != grants_.end()) {
            grants = it->second;
            has_grant = true;
        }
    }
    if (!has_grant) {
        decision.allowed = false;
        decision.reason = "no grants registered for subject '" + subject_key + "'";
        decision.policy_ref = kRefDefaultDeny;
        (void)RecordDenial(request.project_id, request.kind, request.subject, decision.reason);
        return decision;
    }

    std::string deny_reason;
    bool allowed = false;
    switch (request.kind) {
        case ActionKind::ReadPath:
            allowed = PathWithinAny(request.path, grants.readable_roots, &deny_reason);
            break;
        case ActionKind::WritePath:
            allowed = PathWithinAny(request.path, grants.writable_roots, &deny_reason);
            break;
        case ActionKind::PublishFile:
            allowed = PathWithinAny(request.path, grants.publish_roots, &deny_reason);
            break;
        case ActionKind::LaunchProcess:
            allowed = grants.allow_process_launch;
            if (!allowed) deny_reason = "process launch is not granted";
            break;
        case ActionKind::AgentSession:
            allowed = grants.allow_agent_session;
            if (!allowed) deny_reason = "agent session is not granted";
            break;
        case ActionKind::NativeWebTool:
            allowed = grants.allow_native_web_tools;
            if (!allowed) deny_reason = "native web tools are not granted";
            break;
        case ActionKind::MaterialAction:
            if (request.operation_id.empty()) {
                deny_reason = "material action requires an operation identity";
            } else {
                allowed = true;
            }
            break;
    }

    if (allowed) {
        decision.allowed = true;
        decision.reason.clear();
        decision.policy_ref = grants.authority_ref.empty() ? std::string(kRefGrant)
                                                           : grants.authority_ref;
        return decision;
    }

    decision.allowed = false;
    decision.reason = deny_reason.empty() ? "action is not covered by the effective grants"
                                          : deny_reason;
    decision.policy_ref = kRefDefaultDeny;
    (void)RecordDenial(request.project_id, request.kind, request.subject, decision.reason);
    return decision;
}

Status Engine::RecordDenial(const std::string& project_id, ActionKind kind,
                            const nlohmann::json& subject, const std::string& reason) {
    if (!store_) return Status::Error(ErrorCode::Internal, "policy engine has no store");
    EnsureSchema(store_);
    nlohmann::json subject_value = subject.is_null() ? nlohmann::json::object() : subject;
    return store_->Exec(
        "INSERT INTO policy_denials(denial_id, project_id, action, subject, reason, created_at) "
        "VALUES(?,?,?,?,?,?);",
        {SqlValue::Text(NewId("denial")), SqlValue::Text(project_id),
         SqlValue::Text(ActionKindName(kind)), SqlValue::Text(subject_value.dump()),
         SqlValue::Text(RedactSecrets(reason)), SqlValue::Text(NowUtcIso8601())});
}

Status Engine::Require(const ActionRequest& request) {
    AuthorizationDecision decision = Authorize(request);
    if (decision.allowed) return Status::Ok();
    return Status::Error(ErrorCode::Denied, decision.reason);
}

Expected<std::vector<nlohmann::json>> Engine::DenialsFor(const std::string& project_id) {
    if (!store_) {
        return Fail<std::vector<nlohmann::json>>(ErrorCode::Internal, "policy engine has no store");
    }
    EnsureSchema(store_);
    auto rows = store_->Query(
        "SELECT denial_id, action, subject, reason, created_at FROM policy_denials "
        "WHERE project_id = ? ORDER BY created_at ASC, denial_id ASC;",
        {SqlValue::Text(project_id)});
    if (!rows.ok()) return Expected<std::vector<nlohmann::json>>(rows.status());
    std::vector<nlohmann::json> denials;
    for (const auto& row : rows.value()) {
        nlohmann::json subject = nlohmann::json::object();
        auto parsed = ParseJsonBounded(row.Text("subject"));
        if (parsed.ok()) subject = parsed.value();
        denials.push_back({{"denial_id", row.Text("denial_id")},
                           {"action", row.Text("action")},
                           {"subject", subject},
                           {"reason", row.Text("reason")},
                           {"created_at", row.Text("created_at")}});
    }
    return denials;
}

}  // namespace mayasaba::policy
