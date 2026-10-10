// Context Synchronizer (layer 6): immutable snapshots, versions, digests and staleness.
//
// Snapshots are built from the Workspace Manager's manifest plus bounded, authorized content
// references. Content sent onward is filtered for secret-shaped data; repository text is
// untrusted source material and never controller instructions. Changed hashes invalidate
// affected entries and dependents; stale context is rejected, never silently reused.
#pragma once

#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/workspace.hpp"

namespace mayasaba::context {

struct SnapshotEntry {
    std::string rel_path;
    std::string sha256;
    std::uint64_t size = 0;
    std::string coverage;               // from the manifest; CONTENT_INSPECTED requires evidence
};

struct Snapshot {
    std::string snapshot_id;
    std::string project_id;
    std::int64_t epoch = 0;
    std::string kind;                   // REPOSITORY | ATTACHMENT_SET | COUNCIL
    std::string manifest_digest;
    std::string digest;                 // MCB-1 SHA-256 over the snapshot identity+entries
    std::vector<SnapshotEntry> entries;
    std::size_t excluded_count = 0;
    bool truncated = false;
    std::string created_at;
};

struct ContentReference {
    std::string rel_path;
    std::string sha256;
    std::string content;                // bounded, secret-filtered excerpt when readable
    bool truncated = false;
    std::string unavailable_reason;     // binary / too large / unreadable / secret-filtered
};

class Synchronizer {
public:
    Synchronizer(storage::Store* store, std::string canonical_root)
        : store_(store), canonical_root_(std::move(canonical_root)) {}

    // Creates and persists an immutable snapshot from a manifest. Entries carry the manifest
    // digest; the snapshot digest is computed over the persisted entry set.
    Expected<Snapshot> CreateSnapshot(const std::string& project_id, std::int64_t epoch,
                                      const workspace::Manifest& manifest,
                                      const std::string& kind = "REPOSITORY");

    // Loads a persisted snapshot (entries included).
    Expected<Snapshot> LoadSnapshot(const std::string& snapshot_id);

    // Verifies the current filesystem still matches the snapshot: returns the list of changed
    // or missing relative paths (empty means FRESH). Mutates nothing.
    Expected<std::vector<std::string>> StaleEntries(const Snapshot& snapshot);

    // Reads one entry's content bounded by max_bytes with secret filtering. Returns an
    // unavailable_reason instead of failing when the content cannot be safely delivered.
    Expected<ContentReference> ReadContent(const Snapshot& snapshot, const std::string& rel_path,
                                           std::size_t max_bytes = 256 * 1024);

    // Smallest sufficient context for a task: entries matching the given relative prefixes,
    // bounded by budget bytes, each verified against the snapshot hash.
    Expected<std::vector<ContentReference>> ReadTaskContext(const Snapshot& snapshot,
                                                            const std::vector<std::string>& prefixes,
                                                            std::size_t budget_bytes = 1u << 20);

    // Advisory dependency edges derived from observed content (includes/imports), with
    // provenance and an unknown marker. Never a second source of truth.
    struct DependencyEdge {
        std::string from_path;
        std::string to_path;
        std::string method;             // "include-scan"
        std::string snapshot_id;
        bool uncertain = true;
    };
    std::vector<DependencyEdge> AdvisoryDependencies(const Snapshot& snapshot,
                                                     const std::vector<ContentReference>& refs);

    // Incremental invalidation: given a previous snapshot and the current manifest, returns
    // changed paths plus reverse-dependency dependents that must be re-read.
    Expected<std::vector<std::string>> InvalidatedPaths(const Snapshot& previous,
                                                        const workspace::Manifest& current,
                                                        bool include_reverse_dependencies = true);

private:
    storage::Store* store_;
    std::string canonical_root_;
};

// Secret-shaped content filtering for content delivered to agents. Returns true when the
// excerpt was withheld entirely.
bool ShouldWithholdContent(const std::string& content, std::string* reason);

}  // namespace mayasaba::context
