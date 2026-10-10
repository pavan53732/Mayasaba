// Workspace Manager — isolated staging, change set computation and integration candidate.
//
// Implementation happens in a controller-owned staged copy derived from the canonical root.
// The canonical root is only ever read here: CreateStaging copies the authorized subset, and
// the staged baseline (per-file digests) is recorded inside the staging directory so that
// ComputeChangeSet and BuildIntegrationCandidate are self-contained and store-independent.
//
// The Git-worktree path is intentionally not used: house rules forbid Git mutations
// (`git worktree add`) and the copy path must work for non-Git folders too.
#include "mayasaba/workspace.hpp"

#include <algorithm>
#include <cstdint>
#include <map>
#include <string>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::workspace {
namespace {

constexpr const char* kMetaDir = ".mayasaba";
constexpr const char* kBaselineFile = "baseline.json";

struct BaselineEntry {
    std::string rel_path;
    std::string kind;   // FILE | DIRECTORY | LINK
    std::uint64_t size = 0;
    std::string sha256;
};

std::string BaselinePath(const std::string& staging_dir) {
    return fs::JoinPath(fs::JoinPath(staging_dir, kMetaDir), kBaselineFile);
}

void WalkStaged(const std::string& directory, const std::string& rel_prefix,
                std::vector<BaselineEntry>& out) {
    auto listing = fs::ListDirectory(directory);
    if (!listing.ok()) return;
    std::vector<fs::DirectoryEntry> entries = listing.value();
    std::sort(entries.begin(), entries.end(),
              [](const fs::DirectoryEntry& a, const fs::DirectoryEntry& b) { return a.name < b.name; });
    for (const auto& entry : entries) {
        if (entry.name == kMetaDir) continue;
        BaselineEntry record;
        record.rel_path = rel_prefix.empty() ? entry.name : rel_prefix + "/" + entry.name;
        record.size = entry.size;
        const std::string absolute = fs::JoinPath(directory, entry.name);
        if (entry.is_reparse_point) {
            record.kind = "LINK";
            out.push_back(std::move(record));
            continue;
        }
        if (entry.is_directory) {
            record.kind = "DIRECTORY";
            out.push_back(record);
            WalkStaged(absolute, record.rel_path, out);
            continue;
        }
        record.kind = "FILE";
        auto hash = Sha256::HexOfFile(absolute);
        if (hash.ok()) record.sha256 = hash.value();
        out.push_back(std::move(record));
    }
}

std::string DigestOf(const std::vector<BaselineEntry>& entries) {
    nlohmann::json array = nlohmann::json::array();
    for (const auto& entry : entries) {
        array.push_back({{"kind", entry.kind},
                         {"rel_path", entry.rel_path},
                         {"sha256", entry.sha256},
                         {"size", entry.size}});
    }
    auto digest = CanonicalDigest(array);
    return digest.ok() ? digest.value() : std::string();
}

std::map<std::string, BaselineEntry> IndexByPath(const std::vector<BaselineEntry>& entries) {
    std::map<std::string, BaselineEntry> index;
    for (const auto& entry : entries) index[entry.rel_path] = entry;
    return index;
}

}  // namespace

Expected<StagedWorkspace> CreateStaging(const std::string& project_id, const std::string& task_id,
                                        const std::string& canonical_root,
                                        const std::vector<std::string>& allowed_relative_paths,
                                        const std::string& workspace_root) {
    RootValidation root = ValidateRoot(canonical_root);
    if (!root.ok) {
        return Fail<StagedWorkspace>(ErrorCode::InvalidArgument, "invalid canonical root: " + root.reason);
    }
    if (workspace_root.empty()) {
        return Fail<StagedWorkspace>(ErrorCode::InvalidArgument, "workspace root is empty");
    }
    auto ensure = fs::EnsureDirectory(workspace_root);
    if (!ensure.ok()) return Fail<StagedWorkspace>(ensure.code(), ensure.message());

    StagedWorkspace staged;
    staged.workspace_view_id = NewId("wsview");
    staged.project_id = project_id;
    staged.task_id = task_id;
    staged.created_at = NowUtcIso8601();
    staged.path = fs::JoinPath(workspace_root, "staging-" + staged.workspace_view_id);
    auto make_dir = fs::EnsureDirectory(staged.path);
    if (!make_dir.ok()) return Fail<StagedWorkspace>(make_dir.code(), make_dir.message());

    for (const auto& rel_path : allowed_relative_paths) {
        if (!IsSafeRelativePath(rel_path)) {
            return Fail<StagedWorkspace>(ErrorCode::InvalidArgument,
                                         "unsafe relative path refused: " + rel_path);
        }
        const std::string source = fs::JoinPath(root.canonical_path, rel_path);
        if (!fs::IsPathWithin(root.canonical_path, source)) {
            return Fail<StagedWorkspace>(ErrorCode::Denied,
                                         "relative path escapes the canonical root: " + rel_path);
        }
        auto identity = fs::IdentifyPath(source);
        if (!identity.ok() || !identity.value().exists) continue;  // authorized but absent
        const std::string destination = fs::JoinPath(staged.path, rel_path);
        if (identity.value().is_directory) {
            auto copied = fs::CopyTree(source, destination);
            if (!copied.ok()) return Fail<StagedWorkspace>(copied.code(), copied.message());
        } else {
            auto bytes = fs::ReadFileBytes(source);
            if (!bytes.ok()) return Fail<StagedWorkspace>(bytes.code(), bytes.message());
            auto written = fs::WriteFileBytes(destination, bytes.value());
            if (!written.ok()) return Fail<StagedWorkspace>(written.code(), written.message());
        }
    }

    std::vector<BaselineEntry> entries;
    WalkStaged(staged.path, "", entries);
    std::sort(entries.begin(), entries.end(),
              [](const BaselineEntry& a, const BaselineEntry& b) { return a.rel_path < b.rel_path; });
    staged.baseline_digest = DigestOf(entries);

    nlohmann::json baseline = {{"baseline_digest", staged.baseline_digest},
                               {"created_at", staged.created_at},
                               {"entries", nlohmann::json::array()}};
    for (const auto& entry : entries) {
        baseline["entries"].push_back({{"kind", entry.kind},
                                       {"rel_path", entry.rel_path},
                                       {"sha256", entry.sha256},
                                       {"size", entry.size}});
    }
    auto meta = fs::EnsureDirectory(fs::JoinPath(staged.path, kMetaDir));
    if (!meta.ok()) return Fail<StagedWorkspace>(meta.code(), meta.message());
    auto written = fs::WriteFileText(BaselinePath(staged.path), baseline.dump());
    if (!written.ok()) return Fail<StagedWorkspace>(written.code(), written.message());

    return staged;
}

Expected<std::vector<ChangeSetEntry>> ComputeChangeSet(const StagedWorkspace& staged) {
    auto bytes = fs::ReadFileBytes(BaselinePath(staged.path));
    if (!bytes.ok()) {
        return Fail<std::vector<ChangeSetEntry>>(ErrorCode::NotFound,
                                                 "staging baseline is missing: " + bytes.message());
    }
    std::string text(bytes.value().begin(), bytes.value().end());
    auto parsed = ParseJsonBounded(text);
    if (!parsed.ok()) {
        return Fail<std::vector<ChangeSetEntry>>(parsed.code(), parsed.message());
    }
    std::vector<BaselineEntry> baseline_entries;
    if (parsed.value().contains("entries") && parsed.value()["entries"].is_array()) {
        for (const auto& item : parsed.value()["entries"]) {
            BaselineEntry entry;
            entry.rel_path = item.value("rel_path", std::string());
            entry.kind = item.value("kind", std::string());
            entry.size = item.value("size", static_cast<std::uint64_t>(0));
            entry.sha256 = item.value("sha256", std::string());
            baseline_entries.push_back(std::move(entry));
        }
    }
    const auto baseline = IndexByPath(baseline_entries);

    std::vector<BaselineEntry> current_entries;
    WalkStaged(staged.path, "", current_entries);
    const auto current = IndexByPath(current_entries);

    std::vector<ChangeSetEntry> changes;
    for (const auto& [rel_path, entry] : current) {
        if (entry.kind != "FILE") continue;
        auto it = baseline.find(rel_path);
        if (it == baseline.end()) {
            changes.push_back({rel_path, "CREATE", std::string(), entry.sha256, entry.size});
        } else if (it->second.sha256 != entry.sha256) {
            changes.push_back({rel_path, "MODIFY", it->second.sha256, entry.sha256, entry.size});
        }
    }
    for (const auto& [rel_path, entry] : baseline) {
        if (entry.kind != "FILE") continue;
        if (current.find(rel_path) == current.end()) {
            changes.push_back({rel_path, "DELETE", entry.sha256, std::string(), 0});
        }
    }
    std::sort(changes.begin(), changes.end(),
              [](const ChangeSetEntry& a, const ChangeSetEntry& b) { return a.rel_path < b.rel_path; });
    return changes;
}

Expected<IntegrationCandidate> BuildIntegrationCandidate(const StagedWorkspace& staged,
                                                         const std::string& canonical_root) {
    RootValidation root = ValidateRoot(canonical_root);
    if (!root.ok) {
        return Fail<IntegrationCandidate>(ErrorCode::InvalidArgument,
                                          "invalid canonical root: " + root.reason);
    }
    auto changes = ComputeChangeSet(staged);
    if (!changes.ok()) return Fail<IntegrationCandidate>(changes.code(), changes.message());

    IntegrationCandidate candidate;
    candidate.candidate_id = NewId("cand");
    candidate.project_id = staged.project_id;
    candidate.task_id = staged.task_id;
    candidate.changes = changes.value();
    candidate.created_at = NowUtcIso8601();

    for (const auto& change : candidate.changes) {
        const std::string destination = fs::JoinPath(root.canonical_path, change.rel_path);
        std::string current_hash;
        auto identity = fs::IdentifyPath(destination);
        if (identity.ok() && identity.value().exists && !identity.value().is_directory) {
            auto hash = Sha256::HexOfFile(destination);
            if (hash.ok()) current_hash = hash.value();
        }
        // A file that changed in the root since the baseline is a conflict, never overwritten.
        if (current_hash != change.baseline_sha256) {
            candidate.conflicts.push_back(change.rel_path);
        }
    }
    return candidate;
}

}  // namespace mayasaba::workspace
