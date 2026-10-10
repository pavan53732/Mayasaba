// Context Synchronizer (layer 6): immutable snapshots, digests, staleness and secret filtering.
//
// Snapshots are built from the Workspace Manager's manifest. Content delivered onward is
// bounded and filtered for secret-shaped data; repository text is untrusted source material
// and never controller instructions. A changed hash invalidates affected entries and their
// reverse-dependency dependents; stale content is reported, never silently served.
//
// Consolidation note (for the parent): the tables used here (snapshots, snapshot_entries)
// already exist in the v1 storage migration; EnsureSchema() re-asserts them with
// `CREATE TABLE IF NOT EXISTS` lazily as a defensive measure and adds no schema change.
#include "mayasaba/context.hpp"

#include <windows.h>

#include <algorithm>
#include <map>
#include <regex>
#include <set>
#include <sstream>
#include <string>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::context {
namespace {

using storage::SqlValue;

// Reads at most `max_bytes` from the head of a file (bounded delivery for oversized entries).
Expected<std::vector<std::uint8_t>> ReadFilePrefix(const std::string& path, std::size_t max_bytes) {
    const std::wstring wide = Utf8ToWide(path);
    HANDLE handle = CreateFileW(wide.c_str(), GENERIC_READ,
                                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        return Fail<std::vector<std::uint8_t>>(ErrorCode::IoError, "cannot open: " + path);
    }
    std::vector<std::uint8_t> bytes(max_bytes);
    std::size_t offset = 0;
    while (offset < max_bytes) {
        const DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(max_bytes - offset, 1u << 20));
        DWORD read = 0;
        if (!ReadFile(handle, bytes.data() + offset, chunk, &read, nullptr)) {
            CloseHandle(handle);
            return Fail<std::vector<std::uint8_t>>(ErrorCode::IoError, "read failed: " + path);
        }
        if (read == 0) break;
        offset += read;
    }
    CloseHandle(handle);
    bytes.resize(offset);
    return bytes;
}

void EnsureSchema(storage::Store* store) {
    if (!store) return;
    static std::once_flag once;
    std::call_once(once, [store]() {
        (void)store->Exec(
            "CREATE TABLE IF NOT EXISTS snapshots("
            "snapshot_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, epoch INTEGER NOT NULL, "
            "kind TEXT NOT NULL, manifest_digest TEXT NOT NULL DEFAULT '', digest TEXT NOT NULL, "
            "entry_count INTEGER NOT NULL DEFAULT 0, excluded_count INTEGER NOT NULL DEFAULT 0, "
            "truncated INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);");
        (void)store->Exec(
            "CREATE TABLE IF NOT EXISTS snapshot_entries("
            "snapshot_id TEXT NOT NULL, rel_path TEXT NOT NULL, kind TEXT NOT NULL, "
            "size INTEGER NOT NULL DEFAULT 0, sha256 TEXT NOT NULL DEFAULT '', "
            "coverage TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', "
            "PRIMARY KEY(snapshot_id, rel_path));");
    });
}

Expected<std::string> SnapshotDigest(const Snapshot& snapshot) {
    nlohmann::json entries = nlohmann::json::array();
    for (const auto& entry : snapshot.entries) {
        entries.push_back({{"coverage", entry.coverage},
                           {"rel_path", entry.rel_path},
                           {"sha256", entry.sha256},
                           {"size", entry.size}});
    }
    nlohmann::json input = {{"entries", entries},
                            {"epoch", snapshot.epoch},
                            {"kind", snapshot.kind},
                            {"manifest_digest", snapshot.manifest_digest},
                            {"project_id", snapshot.project_id}};
    return CanonicalDigest(input);
}

std::string BaseName(const std::string& path) {
    auto pos = path.find_last_of("/\\");
    return pos == std::string::npos ? path : path.substr(pos + 1);
}

const std::regex& IncludePattern() {
    static const std::regex pattern(R"(#\s*include\s*["<]([^">]+)[">])");
    return pattern;
}

bool PrefixMatches(const std::string& rel_path, const std::string& raw_prefix) {
    std::string prefix = raw_prefix;
    while (!prefix.empty() && (prefix.back() == '/' || prefix.back() == '\\')) prefix.pop_back();
    if (prefix.empty()) return true;
    if (rel_path == prefix) return true;
    if (rel_path.size() > prefix.size() && rel_path.compare(0, prefix.size(), prefix) == 0) {
        return rel_path[prefix.size()] == '/' || rel_path[prefix.size()] == '\\';
    }
    return false;
}

}  // namespace

bool ShouldWithholdContent(const std::string& content, std::string* reason) {
    if (content.empty()) return false;
    struct Rule {
        const char* pattern;
        const char* label;
    };
    static const Rule kRules[] = {
        {R"(\bsk-[A-Za-z0-9_\-]{8,})", "OpenAI-style API key"},
        {R"(\bghp_[A-Za-z0-9]{8,})", "GitHub personal access token"},
        {R"(\bAKIA[0-9A-Z]{12,})", "AWS access key id"},
        {R"(\bxox[baprs]-[A-Za-z0-9\-]{8,})", "Slack token"},
        {R"(-----BEGIN [A-Z ]*PRIVATE KEY-----)", "private key block"},
        {R"((password|passwd|secret|api[_\-]?key|access[_\-]?key)\s*[:=]\s*["'][^"']{6,}["'])",
         "quoted secret assignment"},
        {R"((password|passwd|secret|api[_\-]?key|access[_\-]?key)\s*[:=]\s*[^\s"']{8,})",
         "secret assignment"},
    };
    static const std::vector<std::regex> kCompiled = [] {
        std::vector<std::regex> compiled;
        for (const auto& rule : kRules) {
            compiled.emplace_back(rule.pattern, std::regex::icase);
        }
        return compiled;
    }();

    for (std::size_t i = 0; i < kCompiled.size(); ++i) {
        if (std::regex_search(content, kCompiled[i])) {
            if (reason) *reason = std::string("withheld: secret-shaped content (") + kRules[i].label + ")";
            return true;
        }
    }
    return false;
}

Expected<Snapshot> Synchronizer::CreateSnapshot(const std::string& project_id, std::int64_t epoch,
                                                const workspace::Manifest& manifest,
                                                const std::string& kind) {
    if (!store_) return Fail<Snapshot>(ErrorCode::Internal, "synchronizer has no store");
    EnsureSchema(store_);

    Snapshot snapshot;
    snapshot.snapshot_id = NewId("snap");
    snapshot.project_id = project_id;
    snapshot.epoch = epoch;
    snapshot.kind = kind.empty() ? std::string("REPOSITORY") : kind;
    snapshot.manifest_digest = manifest.digest;
    snapshot.excluded_count = manifest.excluded;
    snapshot.truncated = manifest.truncated;
    snapshot.created_at = NowUtcIso8601();

    struct Pair {
        SnapshotEntry entry;
        std::string manifest_kind;
    };
    std::vector<Pair> pairs;
    pairs.reserve(manifest.entries.size());
    for (const auto& entry : manifest.entries) {
        pairs.push_back({{entry.rel_path, entry.sha256, entry.size, entry.coverage}, entry.kind});
    }
    std::sort(pairs.begin(), pairs.end(),
              [](const Pair& a, const Pair& b) { return a.entry.rel_path < b.entry.rel_path; });
    for (const auto& pair : pairs) snapshot.entries.push_back(pair.entry);

    auto digest = SnapshotDigest(snapshot);
    if (!digest.ok()) return Fail<Snapshot>(digest.code(), digest.message());
    snapshot.digest = digest.value();

    auto insert = store_->Exec(
        "INSERT OR REPLACE INTO snapshots(snapshot_id, project_id, epoch, kind, manifest_digest, "
        "digest, entry_count, excluded_count, truncated, created_at) VALUES(?,?,?,?,?,?,?,?,?,?);",
        {SqlValue::Text(snapshot.snapshot_id), SqlValue::Text(snapshot.project_id),
         SqlValue::Int(snapshot.epoch), SqlValue::Text(snapshot.kind),
         SqlValue::Text(snapshot.manifest_digest), SqlValue::Text(snapshot.digest),
         SqlValue::Int(static_cast<std::int64_t>(snapshot.entries.size())),
         SqlValue::Int(static_cast<std::int64_t>(snapshot.excluded_count)),
         SqlValue::Int(snapshot.truncated ? 1 : 0), SqlValue::Text(snapshot.created_at)});
    if (!insert.ok()) return Fail<Snapshot>(insert.code(), insert.message());

    for (const auto& pair : pairs) {
        auto entry_insert = store_->Exec(
            "INSERT OR REPLACE INTO snapshot_entries(snapshot_id, rel_path, kind, size, sha256, "
            "coverage, reason) VALUES(?,?,?,?,?,?,?);",
            {SqlValue::Text(snapshot.snapshot_id), SqlValue::Text(pair.entry.rel_path),
             SqlValue::Text(pair.manifest_kind),
             SqlValue::Int(static_cast<std::int64_t>(pair.entry.size)),
             SqlValue::Text(pair.entry.sha256), SqlValue::Text(pair.entry.coverage),
             SqlValue::Text("")});
        if (!entry_insert.ok()) return Fail<Snapshot>(entry_insert.code(), entry_insert.message());
    }
    return snapshot;
}

Expected<Snapshot> Synchronizer::LoadSnapshot(const std::string& snapshot_id) {
    if (!store_) return Fail<Snapshot>(ErrorCode::Internal, "synchronizer has no store");
    EnsureSchema(store_);
    auto rows = store_->Query(
        "SELECT snapshot_id, project_id, epoch, kind, manifest_digest, digest, excluded_count, "
        "truncated, created_at FROM snapshots WHERE snapshot_id=?;",
        {SqlValue::Text(snapshot_id)});
    if (!rows.ok()) return Fail<Snapshot>(rows.code(), rows.message());
    if (rows.value().empty()) {
        return Fail<Snapshot>(ErrorCode::NotFound, "snapshot not found: " + snapshot_id);
    }
    const auto& row = rows.value()[0];
    Snapshot snapshot;
    snapshot.snapshot_id = row.Text("snapshot_id");
    snapshot.project_id = row.Text("project_id");
    snapshot.epoch = row.Int("epoch");
    snapshot.kind = row.Text("kind");
    snapshot.manifest_digest = row.Text("manifest_digest");
    snapshot.digest = row.Text("digest");
    snapshot.excluded_count = static_cast<std::size_t>(row.Int("excluded_count"));
    snapshot.truncated = row.Int("truncated") != 0;
    snapshot.created_at = row.Text("created_at");

    auto entries = store_->Query(
        "SELECT rel_path, size, sha256, coverage FROM snapshot_entries WHERE snapshot_id=? "
        "ORDER BY rel_path ASC;",
        {SqlValue::Text(snapshot_id)});
    if (!entries.ok()) return Fail<Snapshot>(entries.code(), entries.message());
    for (const auto& entry : entries.value()) {
        snapshot.entries.push_back({entry.Text("rel_path"), entry.Text("sha256"),
                                    static_cast<std::uint64_t>(entry.Int("size")),
                                    entry.Text("coverage")});
    }
    return snapshot;
}

Expected<std::vector<std::string>> Synchronizer::StaleEntries(const Snapshot& snapshot) {
    std::vector<std::string> stale;
    for (const auto& entry : snapshot.entries) {
        if (entry.sha256.empty()) continue;  // directories/links/excluded carry no hash
        const std::string absolute = fs::JoinPath(canonical_root_, entry.rel_path);
        auto identity = fs::IdentifyPath(absolute);
        if (!identity.ok() || !identity.value().exists || identity.value().is_directory) {
            stale.push_back(entry.rel_path);
            continue;
        }
        auto hash = Sha256::HexOfFile(absolute);
        if (!hash.ok() || hash.value() != entry.sha256) stale.push_back(entry.rel_path);
    }
    std::sort(stale.begin(), stale.end());
    stale.erase(std::unique(stale.begin(), stale.end()), stale.end());
    return stale;
}

Expected<ContentReference> Synchronizer::ReadContent(const Snapshot& snapshot,
                                                     const std::string& rel_path,
                                                     std::size_t max_bytes) {
    ContentReference reference;
    reference.rel_path = rel_path;

    const SnapshotEntry* entry = nullptr;
    for (const auto& candidate : snapshot.entries) {
        if (candidate.rel_path == rel_path) {
            entry = &candidate;
            break;
        }
    }
    if (!entry) {
        reference.unavailable_reason = "path is not part of the snapshot";
        return reference;
    }
    reference.sha256 = entry->sha256;

    const std::string absolute = fs::JoinPath(canonical_root_, rel_path);
    auto identity = fs::IdentifyPath(absolute);
    if (!identity.ok() || !identity.value().exists || identity.value().is_directory) {
        reference.unavailable_reason = "file is missing or unreadable";
        return reference;
    }

    const std::uint64_t size = identity.value().size;
    const std::size_t limit = max_bytes == 0 ? 1 : max_bytes;
    reference.truncated = size > limit;
    Expected<std::vector<std::uint8_t>> bytes =
        reference.truncated ? ReadFilePrefix(absolute, limit)
                            : fs::ReadFileBytes(absolute, static_cast<std::size_t>(size));
    if (!bytes.ok()) {
        reference.unavailable_reason = "cannot read file: " + bytes.message();
        return reference;
    }
    std::string content(bytes.value().begin(), bytes.value().end());

    if (content.find('\0') != std::string::npos) {
        reference.unavailable_reason = "binary content is not delivered";
        reference.truncated = false;
        return reference;
    }

    auto full_hash = Sha256::HexOfFile(absolute);
    if (full_hash.ok()) {
        reference.sha256 = full_hash.value();
        if (!entry->sha256.empty() && full_hash.value() != entry->sha256) {
            reference.unavailable_reason = "content changed since snapshot";
            return reference;
        }
    }

    std::string reason;
    if (ShouldWithholdContent(content, &reason)) {
        reference.content.clear();
        reference.unavailable_reason = reason;
        return reference;
    }
    reference.content = std::move(content);
    return reference;
}

Expected<std::vector<ContentReference>> Synchronizer::ReadTaskContext(
    const Snapshot& snapshot, const std::vector<std::string>& prefixes, std::size_t budget_bytes) {
    std::vector<const SnapshotEntry*> matches;
    for (const auto& entry : snapshot.entries) {
        if (entry.sha256.empty()) continue;  // deliverable file entries only
        for (const auto& prefix : prefixes) {
            if (PrefixMatches(entry.rel_path, prefix)) {
                matches.push_back(&entry);
                break;
            }
        }
    }
    std::sort(matches.begin(), matches.end(),
              [](const SnapshotEntry* a, const SnapshotEntry* b) { return a->rel_path < b->rel_path; });

    std::vector<ContentReference> delivered;
    std::size_t used = 0;
    for (const SnapshotEntry* entry : matches) {
        if (used >= budget_bytes) break;
        const std::size_t remaining = budget_bytes - used;
        auto reference = ReadContent(snapshot, entry->rel_path, remaining);
        if (!reference.ok()) continue;
        used += reference.value().content.size();
        delivered.push_back(std::move(reference.value()));
    }
    return delivered;
}

std::vector<Synchronizer::DependencyEdge> Synchronizer::AdvisoryDependencies(
    const Snapshot& snapshot, const std::vector<ContentReference>& refs) {
    std::vector<DependencyEdge> edges;
    for (const auto& reference : refs) {
        if (reference.content.empty()) continue;
        std::istringstream stream(reference.content);
        std::string line;
        while (std::getline(stream, line)) {
            std::smatch match;
            if (std::regex_search(line, match, IncludePattern())) {
                DependencyEdge edge;
                edge.from_path = reference.rel_path;
                edge.to_path = match[1].str();
                edge.method = "include-scan";
                edge.snapshot_id = snapshot.snapshot_id;
                edge.uncertain = true;
                edges.push_back(std::move(edge));
            }
        }
    }
    return edges;
}

Expected<std::vector<std::string>> Synchronizer::InvalidatedPaths(
    const Snapshot& previous, const workspace::Manifest& current, bool include_reverse_dependencies) {
    std::map<std::string, std::string> previous_hashes;
    for (const auto& entry : previous.entries) {
        if (!entry.sha256.empty()) previous_hashes[entry.rel_path] = entry.sha256;
    }
    std::map<std::string, std::string> current_hashes;
    for (const auto& entry : current.entries) {
        if (!entry.sha256.empty()) current_hashes[entry.rel_path] = entry.sha256;
    }

    std::set<std::string> changed;
    for (const auto& [rel_path, hash] : current_hashes) {
        auto it = previous_hashes.find(rel_path);
        if (it == previous_hashes.end() || it->second != hash) changed.insert(rel_path);
    }
    for (const auto& [rel_path, hash] : previous_hashes) {
        if (current_hashes.find(rel_path) == current_hashes.end()) changed.insert(rel_path);
    }

    std::set<std::string> invalidated = changed;
    if (include_reverse_dependencies && !changed.empty()) {
        for (const auto& [rel_path, hash] : current_hashes) {
            const std::string absolute = fs::JoinPath(canonical_root_, rel_path);
            auto bytes = fs::ReadFileBytes(absolute, 1u << 20);
            if (!bytes.ok()) continue;
            std::string text(bytes.value().begin(), bytes.value().end());
            std::istringstream stream(text);
            std::string line;
            bool depends = false;
            while (std::getline(stream, line) && !depends) {
                std::smatch match;
                if (!std::regex_search(line, match, IncludePattern())) continue;
                const std::string target = match[1].str();
                for (const auto& changed_path : changed) {
                    if (target == changed_path || BaseName(target) == BaseName(changed_path)) {
                        depends = true;
                        break;
                    }
                }
            }
            if (depends) invalidated.insert(rel_path);
        }
    }
    return std::vector<std::string>(invalidated.begin(), invalidated.end());
}

}  // namespace mayasaba::context
