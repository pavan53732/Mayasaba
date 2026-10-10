// Workspace Manager — durable publication journal, guarded publication and reconciliation.
//
// The journal row (publications + publication_files) is persisted BEFORE any mutation. Each
// file mutation revalidates the current root hash immediately before touching the file, backs
// the existing bytes up, writes a same-volume temp file and replaces atomically; a file that
// diverged from the baseline is a conflict and is never overwritten. Reconciliation reports
// the truthful per-file state and never rolls back newer user edits.
//
// Interface note (reported to the parent): PublicationPlan carries no staged path and
// ChangeSetEntry has no staged_path field, so the controller-owned staged content is located
// under `recovery_root` (staged file = recovery_root/<rel_path>; backups under
// recovery_root/.recovery/<publication_id>/<rel_path>). See interface_change_requests.
#include "mayasaba/workspace.hpp"

#include <windows.h>

#include <algorithm>
#include <cstdint>
#include <string>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::workspace {
namespace {

using storage::SqlValue;

constexpr const char* kRecoverySubdir = ".recovery";
constexpr const char* kStatePending = "PENDING";
constexpr const char* kStateApplied = "APPLIED";
constexpr const char* kStateConflict = "CONFLICT";
constexpr const char* kStateFailed = "FAILED";
constexpr const char* kStateDiverged = "DIVERGED";

void EnsureSchema(storage::Store* store) {
    if (!store) return;
    static std::once_flag once;
    std::call_once(once, [store]() {
        (void)store->Exec(
            "CREATE TABLE IF NOT EXISTS publications("
            "publication_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, task_id TEXT, "
            "state TEXT NOT NULL, journal TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, "
            "updated_at TEXT NOT NULL);");
        (void)store->Exec(
            "CREATE TABLE IF NOT EXISTS publication_files("
            "publication_id TEXT NOT NULL, rel_path TEXT NOT NULL, operation TEXT NOT NULL, "
            "before_sha256 TEXT, after_sha256 TEXT NOT NULL, staged_path TEXT NOT NULL, "
            "backup_path TEXT, state TEXT NOT NULL DEFAULT 'PENDING', "
            "PRIMARY KEY(publication_id, rel_path));");
    });
}

std::string StagedPathFor(const std::string& recovery_root, const std::string& rel_path) {
    return fs::JoinPath(recovery_root, rel_path);
}

std::string BackupPathFor(const std::string& recovery_root, const std::string& publication_id,
                          const std::string& rel_path) {
    return fs::JoinPath(fs::JoinPath(fs::JoinPath(recovery_root, kRecoverySubdir), publication_id),
                        rel_path);
}

// Hash of the current destination file ("" when absent or a directory).
std::string CurrentHash(const std::string& path) {
    auto identity = fs::IdentifyPath(path);
    if (!identity.ok() || !identity.value().exists || identity.value().is_directory) return std::string();
    auto hash = Sha256::HexOfFile(path);
    return hash.ok() ? hash.value() : std::string();
}

Status MarkFileState(storage::Store* store, const std::string& publication_id,
                     const std::string& rel_path, const std::string& state) {
    return store->Exec(
        "UPDATE publication_files SET state=? WHERE publication_id=? AND rel_path=?;",
        {SqlValue::Text(state), SqlValue::Text(publication_id), SqlValue::Text(rel_path)});
}

}  // namespace

Expected<PublicationOutcome> Publish(const PublicationPlan& plan, const std::string& canonical_root,
                                     storage::Store* store, const std::string& recovery_root) {
    if (!store) return Fail<PublicationOutcome>(ErrorCode::Internal, "publication requires a store");
    if (plan.publication_id.empty()) {
        return Fail<PublicationOutcome>(ErrorCode::InvalidArgument, "publication id is required");
    }
    RootValidation root = ValidateRoot(canonical_root);
    if (!root.ok) {
        return Fail<PublicationOutcome>(ErrorCode::InvalidArgument, "invalid canonical root: " + root.reason);
    }
    if (recovery_root.empty()) {
        return Fail<PublicationOutcome>(ErrorCode::InvalidArgument, "recovery root is required");
    }
    auto ensure = fs::EnsureDirectory(recovery_root);
    if (!ensure.ok()) return Fail<PublicationOutcome>(ensure.code(), ensure.message());
    EnsureSchema(store);

    PublicationOutcome outcome;
    outcome.publication_id = plan.publication_id;
    const std::string now = NowUtcIso8601();

    // 1. Persist the durable journal BEFORE any mutation.
    nlohmann::json journal = {{"canonical_root", root.canonical_path},
                              {"recovery_root", recovery_root},
                              {"changes", nlohmann::json::array()}};
    for (const auto& change : plan.changes) {
        journal["changes"].push_back({{"rel_path", change.rel_path},
                                      {"operation", change.operation},
                                      {"baseline_sha256", change.baseline_sha256},
                                      {"result_sha256", change.result_sha256}});
    }
    auto insert_publication = store->Exec(
        "INSERT OR REPLACE INTO publications(publication_id, project_id, task_id, state, journal, "
        "created_at, updated_at) VALUES(?,?,?,?,?,?,?);",
        {SqlValue::Text(plan.publication_id), SqlValue::Text(plan.project_id),
         SqlValue::Text(plan.task_id), SqlValue::Text(kStatePending), SqlValue::Text(journal.dump()),
         SqlValue::Text(now), SqlValue::Text(now)});
    if (!insert_publication.ok()) {
        return Fail<PublicationOutcome>(insert_publication.code(), insert_publication.message());
    }
    for (const auto& change : plan.changes) {
        auto insert_file = store->Exec(
            "INSERT OR REPLACE INTO publication_files(publication_id, rel_path, operation, "
            "before_sha256, after_sha256, staged_path, backup_path, state) VALUES(?,?,?,?,?,?,?,?);",
            {SqlValue::Text(plan.publication_id), SqlValue::Text(change.rel_path),
             SqlValue::Text(change.operation), SqlValue::Text(change.baseline_sha256),
             SqlValue::Text(change.result_sha256),
             SqlValue::Text(StagedPathFor(recovery_root, change.rel_path)),
             SqlValue::Text(BackupPathFor(recovery_root, plan.publication_id, change.rel_path)),
             SqlValue::Text(kStatePending)});
        if (!insert_file.ok()) {
            return Fail<PublicationOutcome>(insert_file.code(), insert_file.message());
        }
    }

    // 2. Apply guarded, same-volume file operations.
    for (const auto& change : plan.changes) {
        const std::string destination = fs::JoinPath(root.canonical_path, change.rel_path);
        const std::string current = CurrentHash(destination);
        const bool destination_exists = !current.empty();

        // Revalidate immediately before mutating: a concurrent edit is a conflict.
        if (current != change.baseline_sha256) {
            outcome.conflicted_paths.push_back(change.rel_path);
            (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateConflict);
            continue;
        }

        if (destination_exists) {
            auto existing = fs::ReadFileBytes(destination);
            if (existing.ok()) {
                (void)fs::WriteFileBytes(
                    BackupPathFor(recovery_root, plan.publication_id, change.rel_path),
                    existing.value());
            }
        }

        if (change.operation == "DELETE") {
            auto removed = fs::RemoveFile(destination);
            if (!removed.ok()) {
                outcome.failed_paths.push_back(change.rel_path);
                outcome.detail += "delete failed for " + change.rel_path + ": " + removed.message() + "; ";
                (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateFailed);
                continue;
            }
            outcome.applied_paths.push_back(change.rel_path);
            (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateApplied);
            continue;
        }

        // CREATE or MODIFY: read the staged content, write a same-volume temp file and replace.
        auto staged_bytes = fs::ReadFileBytes(StagedPathFor(recovery_root, change.rel_path));
        if (!staged_bytes.ok()) {
            outcome.failed_paths.push_back(change.rel_path);
            outcome.detail += "staged file unreadable for " + change.rel_path + ": " +
                              staged_bytes.message() + "; ";
            (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateFailed);
            continue;
        }
        const std::string temp_path = destination + ".mayasaba-tmp-" + NewId("t");
        auto written = fs::WriteFileBytes(temp_path, staged_bytes.value());
        if (!written.ok()) {
            outcome.failed_paths.push_back(change.rel_path);
            outcome.detail += "temp write failed for " + change.rel_path + ": " + written.message() + "; ";
            (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateFailed);
            continue;
        }
        if (!MoveFileExW(Utf8ToWide(temp_path).c_str(), Utf8ToWide(destination).c_str(),
                         MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
            (void)fs::RemoveFile(temp_path);
            outcome.failed_paths.push_back(change.rel_path);
            outcome.detail += "atomic replace failed for " + change.rel_path + "; ";
            (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateFailed);
            continue;
        }
        const std::string verified = CurrentHash(destination);
        if (verified != change.result_sha256) {
            outcome.failed_paths.push_back(change.rel_path);
            outcome.detail += "post-write hash mismatch for " + change.rel_path + "; ";
            (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateFailed);
            continue;
        }
        outcome.applied_paths.push_back(change.rel_path);
        (void)MarkFileState(store, plan.publication_id, change.rel_path, kStateApplied);
    }

    if (!outcome.failed_paths.empty()) {
        outcome.state = "FAILED";
    } else if (!outcome.conflicted_paths.empty()) {
        outcome.state = "CONFLICT";
    } else {
        outcome.state = "PUBLISHED";
    }
    (void)store->Exec("UPDATE publications SET state=?, updated_at=? WHERE publication_id=?;",
                      {SqlValue::Text(outcome.state), SqlValue::Text(NowUtcIso8601()),
                       SqlValue::Text(plan.publication_id)});
    return outcome;
}

Expected<PublicationOutcome> Reconcile(const std::string& publication_id, storage::Store* store,
                                       const std::string& canonical_root) {
    if (!store) return Fail<PublicationOutcome>(ErrorCode::Internal, "reconcile requires a store");
    EnsureSchema(store);

    auto publication = store->Query(
        "SELECT project_id, task_id, state FROM publications WHERE publication_id=?;",
        {SqlValue::Text(publication_id)});
    if (!publication.ok()) return Expected<PublicationOutcome>(publication.status());
    if (publication.value().empty()) {
        return Fail<PublicationOutcome>(ErrorCode::NotFound, "publication not found: " + publication_id);
    }
    RootValidation root = ValidateRoot(canonical_root);
    if (!root.ok) {
        return Fail<PublicationOutcome>(ErrorCode::InvalidArgument, "invalid canonical root: " + root.reason);
    }

    auto files = store->Query(
        "SELECT rel_path, operation, before_sha256, after_sha256, state FROM publication_files "
        "WHERE publication_id=? ORDER BY rel_path ASC;",
        {SqlValue::Text(publication_id)});
    if (!files.ok()) return Expected<PublicationOutcome>(files.status());

    PublicationOutcome outcome;
    outcome.publication_id = publication_id;
    bool any_pending = false;
    bool any_diverged = false;
    bool any_failed = false;

    for (const auto& row : files.value()) {
        const std::string rel_path = row.Text("rel_path");
        const std::string operation = row.Text("operation");
        const std::string before = row.Text("before_sha256");
        const std::string after = row.Text("after_sha256");
        const std::string recorded = row.Text("state");
        const std::string destination = fs::JoinPath(root.canonical_path, rel_path);
        const std::string current = CurrentHash(destination);
        const bool exists = !current.empty();

        std::string state;
        if (operation == "DELETE") {
            if (!exists) {
                state = kStateApplied;
            } else if (current == before) {
                state = kStatePending;
            } else {
                state = kStateDiverged;
            }
        } else {
            if (exists && current == after) {
                state = kStateApplied;
            } else if (exists && current == before) {
                state = kStatePending;
            } else {
                state = kStateDiverged;
            }
        }
        if (recorded == kStateFailed && state == kStatePending) state = kStateFailed;

        if (state == kStateApplied) {
            outcome.applied_paths.push_back(rel_path);
        } else if (state == kStatePending) {
            any_pending = true;
        } else if (state == kStateDiverged) {
            any_diverged = true;
            outcome.conflicted_paths.push_back(rel_path);
        } else {
            any_failed = true;
            outcome.failed_paths.push_back(rel_path);
        }
        (void)MarkFileState(store, publication_id, rel_path, state);
    }

    if (any_failed) {
        outcome.state = "FAILED";
    } else if (any_diverged) {
        outcome.state = "CONFLICT";
    } else if (any_pending) {
        outcome.state = "RECOVERING";
    } else {
        outcome.state = "PUBLISHED";
    }
    (void)store->Exec("UPDATE publications SET state=?, updated_at=? WHERE publication_id=?;",
                      {SqlValue::Text(outcome.state), SqlValue::Text(NowUtcIso8601()),
                       SqlValue::Text(publication_id)});
    return outcome;
}

Expected<std::vector<nlohmann::json>> PendingPublications(storage::Store* store) {
    if (!store) {
        return Fail<std::vector<nlohmann::json>>(ErrorCode::Internal, "pending publications requires a store");
    }
    EnsureSchema(store);
    auto rows = store->Query(
        "SELECT publication_id, project_id, task_id, state, created_at, updated_at FROM publications "
        "WHERE state NOT IN ('PUBLISHED','FAILED','CONFLICT') ORDER BY created_at ASC, publication_id ASC;");
    if (!rows.ok()) return Expected<std::vector<nlohmann::json>>(rows.status());
    std::vector<nlohmann::json> pending;
    for (const auto& row : rows.value()) {
        pending.push_back({{"publication_id", row.Text("publication_id")},
                           {"project_id", row.Text("project_id")},
                           {"task_id", row.Text("task_id")},
                           {"state", row.Text("state")},
                           {"created_at", row.Text("created_at")},
                           {"updated_at", row.Text("updated_at")}});
    }
    return pending;
}

}  // namespace mayasaba::workspace
