// Context Synchronizer (layer 6): immutable snapshots, digests, staleness and secret filtering.
//
// Snapshots are built from the Workspace Manager's manifest. Content delivered onward is
// bounded and filtered for secret-shaped data; repository text is untrusted source material
// and never controller instructions. A changed hash invalidates affected entries and their
// reverse-dependency dependents; stale content is reported, never silently served.
//
// Schema note: all snapshot persistence lives in the storage layer's migrations
// (core/src/storage/store.cpp). This layer owns no DDL.
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

// The eight registered coverage-provenance states (spec section 2, coverage-accounted
// exploration). A filename listing is INVENTORIED, never CONTENT_INSPECTED.
const std::set<std::string>& AllowedCoverageStates() {
    static const std::set<std::string> kStates = {"INVENTORIED", "CONTENT_INSPECTED", "ANALYZED",
                                                  "EXCLUDED",    "UNREADABLE",        "UNSUPPORTED",
                                                  "TOO_LARGE",   "STALE"};
    return kStates;
}

// Coverage states that must carry a concrete reason and can never be silently delivered.
bool IsNegativeCoverage(const std::string& coverage) {
    return coverage == "EXCLUDED" || coverage == "UNREADABLE" || coverage == "UNSUPPORTED" ||
           coverage == "TOO_LARGE" || coverage == "STALE";
}

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

// Snapshot digest profile version persisted with each snapshot row. Profile 1 is the original
// pre-change shape (entries without "reason", no profile field in the hashed input); profile 2
// tags the hashed input with the profile version and includes each entry's reason. The loader
// recomputes under exactly the persisted profile and fails closed on unknown profiles or any
// mismatch — historical snapshots are never rewritten and tamper detection is never weakened.
constexpr int kSnapshotDigestProfileVersion = 2;

// Recomputes the digest for a snapshot under a specific profile version. V1 omits "reason"
// from the entry hash (matching historical snapshots); V2 includes it.
Expected<std::string> SnapshotDigest(const Snapshot& snapshot, int profile_version) {
    nlohmann::json entries = nlohmann::json::array();
    for (const auto& entry : snapshot.entries) {
        if (profile_version >= 2) {
            entries.push_back({{"coverage", entry.coverage},
                               {"reason", entry.reason},
                               {"rel_path", entry.rel_path},
                               {"sha256", entry.sha256},
                               {"size", entry.size}});
        } else {
            entries.push_back({{"coverage", entry.coverage},
                               {"rel_path", entry.rel_path},
                               {"sha256", entry.sha256},
                               {"size", entry.size}});
        }
    }
    nlohmann::json input = {{"digest_profile", profile_version},
                            {"entries", entries},
                            {"epoch", snapshot.epoch},
                            {"kind", snapshot.kind},
                            {"manifest_digest", snapshot.manifest_digest},
                            {"project_id", snapshot.project_id}};
    return CanonicalDigest(input);
}

// SnapshotDigest_Profile1 computes the digest under the ORIGINAL pre-reason shape: entries,
// epoch, kind, manifest_digest, project_id — NO digest_profile field, NO reason in entries.
// This matches genuine pre-existing snapshots exactly, so legacy rows verify without rewriting
// their history. (SnapshotDigest with an explicit profile_version is used for V2+ content.)
Expected<std::string> SnapshotDigest_Profile1(const Snapshot& snapshot) {
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

// Back-computes the legacy digest (V1 profile) for a snapshot, so a pre-existing snapshot whose
// stored digest was computed without "reason" can still be verified without rewriting its history.
Expected<std::string> LegacySnapshotDigest(const Snapshot& snapshot) {
    return SnapshotDigest_Profile1(snapshot);
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

bool IsValidCoverage(const std::string& coverage) {
    return AllowedCoverageStates().count(coverage) != 0;
}

bool CoverageRequiresReadEvidence(const std::string& coverage) {
    return coverage == "CONTENT_INSPECTED" || coverage == "ANALYZED";
}

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
        {R"(\bbearer\s+[A-Za-z0-9._~+/=\-]{16,})", "bearer token"},
        {R"((mongodb|postgres|postgresql|mysql|mariadb|redis|rediss|amqp|amqps|mssql|sqlserver)(\+\w+)?://[^\s"']+)",
         "connection string URI"},
        {R"((server|data\s*source|host|user\s*id|uid|initial\s*catalog|database)\s*=\s*[^;\s"']+;\s*[^;\s"']*\b(password|pwd)\s*=\s*[^;\s"']+)",
         "connection string with credentials"},
        {R"((password|passwd|secret|api[_\-]?key|access[_\-]?key|client[_\-]?secret|auth[_\-]?token)\s*[:=]\s*["'][^"']{6,}["'])",
         "quoted secret assignment"},
        {R"((password|passwd|secret|api[_\-]?key|access[_\-]?key|client[_\-]?secret|auth[_\-]?token)\s*[:=]\s*[^\s"']{8,})",
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
        // Coverage provenance is validated at the boundary: only the eight registered states
        // are accepted; CONTENT_INSPECTED/ANALYZED require read evidence (a sha256) and the
        // negative states require a concrete reason. Fail closed rather than invent coverage.
        if (!IsValidCoverage(entry.coverage)) {
            return Fail<Snapshot>(ErrorCode::InvalidArgument,
                                  "invalid coverage state '" + entry.coverage + "' for entry '" +
                                      entry.rel_path + "'");
        }
        if (CoverageRequiresReadEvidence(entry.coverage) && entry.sha256.empty()) {
            return Fail<Snapshot>(
                ErrorCode::InvalidArgument,
                "coverage " + entry.coverage + " for entry '" + entry.rel_path +
                    "' requires read evidence (a sha256) but none was supplied");
        }
        if (IsNegativeCoverage(entry.coverage) && entry.reason.empty()) {
            return Fail<Snapshot>(ErrorCode::InvalidArgument,
                                  "coverage " + entry.coverage + " for entry '" + entry.rel_path +
                                      "' requires a concrete reason");
        }
        SnapshotEntry snapshot_entry{entry.rel_path, entry.sha256, entry.size, entry.coverage,
                                     entry.reason};
        pairs.push_back({snapshot_entry, entry.kind});
    }
    std::sort(pairs.begin(), pairs.end(),
              [](const Pair& a, const Pair& b) { return a.entry.rel_path < b.entry.rel_path; });
    for (const auto& pair : pairs) snapshot.entries.push_back(pair.entry);

    auto digest = SnapshotDigest(snapshot, kSnapshotDigestProfileVersion);
    if (!digest.ok()) return Fail<Snapshot>(digest.code(), digest.message());
    snapshot.digest = digest.value();

    // Immutability guard: a snapshot id is write-once. A caller that supplies an existing id
    // must be rejected, and the plain INSERT (no OR REPLACE) is a second line of defence so a
    // persisted snapshot's entries can never be rewritten.
    auto existing = store_->Query("SELECT snapshot_id FROM snapshots WHERE snapshot_id=?;",
                                  {SqlValue::Text(snapshot.snapshot_id)});
    if (!existing.ok()) return Fail<Snapshot>(existing.code(), existing.message());
    if (!existing.value().empty()) {
        return Fail<Snapshot>(ErrorCode::AlreadyExists,
                              "snapshot is immutable and already exists: " + snapshot.snapshot_id);
    }

    auto insert = store_->Exec(
        "INSERT INTO snapshots(snapshot_id, project_id, epoch, kind, manifest_digest, "
        "digest, entry_count, excluded_count, truncated, created_at, digest_profile) "
        "VALUES(?,?,?,?,?,?,?,?,?,?,?);",
        {SqlValue::Text(snapshot.snapshot_id), SqlValue::Text(snapshot.project_id),
         SqlValue::Int(snapshot.epoch), SqlValue::Text(snapshot.kind),
         SqlValue::Text(snapshot.manifest_digest), SqlValue::Text(snapshot.digest),
         SqlValue::Int(static_cast<std::int64_t>(snapshot.entries.size())),
         SqlValue::Int(static_cast<std::int64_t>(snapshot.excluded_count)),
         SqlValue::Int(snapshot.truncated ? 1 : 0), SqlValue::Text(snapshot.created_at),
         SqlValue::Int(kSnapshotDigestProfileVersion)});
    if (!insert.ok()) return Fail<Snapshot>(insert.code(), insert.message());

    for (const auto& pair : pairs) {
        auto entry_insert = store_->Exec(
            "INSERT INTO snapshot_entries(snapshot_id, rel_path, kind, size, sha256, "
            "coverage, reason) VALUES(?,?,?,?,?,?,?);",
            {SqlValue::Text(snapshot.snapshot_id), SqlValue::Text(pair.entry.rel_path),
             SqlValue::Text(pair.manifest_kind),
             SqlValue::Int(static_cast<std::int64_t>(pair.entry.size)),
             SqlValue::Text(pair.entry.sha256), SqlValue::Text(pair.entry.coverage),
             SqlValue::Text(pair.entry.reason)});
        if (!entry_insert.ok()) return Fail<Snapshot>(entry_insert.code(), entry_insert.message());
    }
    return snapshot;
}

Expected<Snapshot> Synchronizer::LoadSnapshot(const std::string& snapshot_id) {
    if (!store_) return Fail<Snapshot>(ErrorCode::Internal, "synchronizer has no store");
    auto rows = store_->Query(
        "SELECT snapshot_id, project_id, epoch, kind, manifest_digest, digest, excluded_count, "
        "truncated, created_at, digest_profile FROM snapshots WHERE snapshot_id=?;",
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
    const int stored_profile = static_cast<int>(row.Int("digest_profile"));

    auto entries = store_->Query(
        "SELECT rel_path, size, sha256, coverage, reason FROM snapshot_entries WHERE snapshot_id=? "
        "ORDER BY rel_path ASC;",
        {SqlValue::Text(snapshot_id)});
    if (!entries.ok()) return Fail<Snapshot>(entries.code(), entries.message());
    for (const auto& entry : entries.value()) {
        const std::string coverage = entry.Text("coverage");
        if (!IsValidCoverage(coverage)) {
            return Fail<Snapshot>(ErrorCode::IntegrityFailure,
                                  "persisted snapshot has an invalid coverage state '" + coverage +
                                      "' for entry '" + entry.Text("rel_path") + "'");
        }
        snapshot.entries.push_back({entry.Text("rel_path"), entry.Text("sha256"),
                                    static_cast<std::uint64_t>(entry.Int("size")), coverage,
                                    entry.Text("reason")});
    }

    // Reloading a snapshot must reproduce the identical digest under the profile persisted with
    // the row. The persisted profile is authoritative: an unknown profile fails closed (we never
    // guess which hash shape produced a stored digest) and there is no cross-profile fallback,
    // so a legacy row can never be silently accepted under V2 rules or vice versa. Tamper
    // detection is preserved: any mismatch under the correct profile fails closed.
    if (stored_profile == kSnapshotDigestProfileVersion) {
        auto recomputed = SnapshotDigest(snapshot, stored_profile);
        if (!recomputed.ok()) return Fail<Snapshot>(recomputed.code(), recomputed.message());
        if (recomputed.value() != snapshot.digest) {
            return Fail<Snapshot>(ErrorCode::IntegrityFailure,
                                  "snapshot digest mismatch: stored " + snapshot.digest +
                                      " but recomputed (profile " +
                                      std::to_string(stored_profile) + ") " + recomputed.value());
        }
    } else if (stored_profile == 1) {
        // Legacy snapshot (profile 1): recompute under the original pre-change shape. A
        // mismatch means the row was corrupted or tampered with after creation — fail closed.
        auto recomputed = LegacySnapshotDigest(snapshot);
        if (!recomputed.ok()) return Fail<Snapshot>(recomputed.code(), recomputed.message());
        if (recomputed.value() != snapshot.digest) {
            return Fail<Snapshot>(ErrorCode::IntegrityFailure,
                                  "snapshot digest mismatch: stored " + snapshot.digest +
                                      " but recomputed (legacy profile 1) " + recomputed.value());
        }
    } else {
        return Fail<Snapshot>(ErrorCode::IntegrityFailure,
                              "unknown snapshot digest profile " + std::to_string(stored_profile) +
                                  " for snapshot " + snapshot_id + "; refusing to guess");
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

    // Coverage provenance fails closed: an entry the snapshot marked EXCLUDED/UNREADABLE/
    // UNSUPPORTED/STALE has no authorized or current content and is never delivered.
    if (entry->coverage == "EXCLUDED" || entry->coverage == "UNREADABLE" ||
        entry->coverage == "UNSUPPORTED" || entry->coverage == "STALE") {
        reference.unavailable_reason =
            "entry is not deliverable (coverage=" + entry->coverage + ")";
        return reference;
    }

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
        reference.unavailable_reason = "secret-filtered: " + reason;
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
        // Build the observed include graph from the current readable files (bounded reads), then
        // compute the reverse-dependency closure: any file that includes an invalidated path,
        // directly or transitively, is itself invalidated. This mirrors the include-scan edges
        // from AdvisoryDependencies (method "include-scan"). The fixpoint loop is bounded by the
        // number of readable files so it always terminates.
        struct Edge {
            std::string from_path;
            std::string target;
        };
        std::vector<Edge> edges;
        std::size_t read_budget = 8u << 20;  // bounded total scan
        for (const auto& [rel_path, hash] : current_hashes) {
            if (read_budget == 0) break;
            const std::string absolute = fs::JoinPath(canonical_root_, rel_path);
            auto bytes =
                fs::ReadFileBytes(absolute, std::min<std::size_t>(1u << 20, read_budget));
            if (!bytes.ok()) continue;
            read_budget -= std::min<std::size_t>(bytes.value().size(), read_budget);
            std::string text(bytes.value().begin(), bytes.value().end());
            std::istringstream stream(text);
            std::string line;
            while (std::getline(stream, line)) {
                std::smatch match;
                if (!std::regex_search(line, match, IncludePattern())) continue;
                edges.push_back({rel_path, match[1].str()});
            }
        }

        bool grew = true;
        std::size_t iterations = 0;
        const std::size_t max_iterations = current_hashes.size() + 1;
        while (grew && iterations++ < max_iterations) {
            grew = false;
            std::vector<std::string> to_add;
            for (const auto& edge : edges) {
                if (invalidated.count(edge.from_path)) continue;
                for (const auto& inv : invalidated) {
                    if (edge.target == inv || BaseName(edge.target) == BaseName(inv)) {
                        to_add.push_back(edge.from_path);
                        break;
                    }
                }
            }
            for (const auto& path : to_add) {
                if (invalidated.insert(path).second) grew = true;
            }
        }
    }
    return std::vector<std::string>(invalidated.begin(), invalidated.end());
}

}  // namespace mayasaba::context
