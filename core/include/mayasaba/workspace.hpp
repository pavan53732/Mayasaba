// Workspace Manager (layer 8): folder boundaries, isolated staging, integration, publication.
//
// The user's selected folder is the canonical root and final destination. Agents never write
// it concurrently: implementation happens in controller-owned isolated staging derived from
// the root, and accepted changes are published through a durable journal with guarded,
// same-volume file operations and reconciliation.
#pragma once

#include <chrono>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::workspace {

// --- Root validation and binding ---------------------------------------------------------

struct RootValidation {
    bool ok = false;
    std::string reason;              // exact rejection reason
    std::string canonical_path;      // resolved canonical path when ok
    std::string display_path;        // user-visible path as chosen
    std::uint64_t volume_serial = 0;
    std::uint64_t file_index = 0;
    bool is_directory = false;
    bool is_reparse_point = false;
};

// Validates a candidate root: existence, locality, directory, reparse handling. Never scans
// contents, never writes, never initializes Git.
RootValidation ValidateRoot(const std::string& candidate_path);

// --- Repository manifest (bounded, read-only) --------------------------------------------

struct ManifestEntry {
    std::string rel_path;            // '/'-separated, relative to root
    std::string kind;                // FILE | DIRECTORY | LINK
    std::uint64_t size = 0;
    std::string sha256;              // present for readable files within budget
    std::string coverage;            // INVENTORIED | EXCLUDED | UNREADABLE | UNSUPPORTED | TOO_LARGE
    std::string reason;              // for excluded/unreadable/oversized entries
};

struct ManifestOptions {
    std::size_t max_files = 50000;          // bounded traversal
    std::size_t max_file_hash_bytes = 4u << 20;
    std::size_t max_depth = 32;
    std::vector<std::string> excluded_names = {".git", "node_modules", ".venv", "venv",
                                               "target", "dist", "build", "__pycache__",
                                               ".kilo", ".workbuddy-ai"};
    std::chrono::milliseconds time_budget{60000};
};

struct Manifest {
    std::string project_id;
    std::string snapshot_id;
    std::string digest;                      // MCB-1 SHA-256 over entries
    std::vector<ManifestEntry> entries;
    std::size_t inventoried = 0;
    std::size_t excluded = 0;
    std::size_t unreadable = 0;
    std::size_t too_large = 0;
    bool truncated = false;                  // traversal budget hit
    std::string created_at;
};

// Enumerates the permitted tree with declared rules and budgets. Junction/reparse entries are
// recorded as LINK and never followed. File mutation between enumeration and hashing is
// detected via identity checks and reported as UNREADABLE/STALE, never silently accepted.
Expected<Manifest> BuildManifest(const std::string& project_id, const std::string& canonical_root,
                                 const ManifestOptions& options = {});

// --- Isolated staging --------------------------------------------------------------------

struct StagedWorkspace {
    std::string workspace_view_id;
    std::string project_id;
    std::string task_id;
    std::string path;                        // controller-owned staging directory
    std::string baseline_digest;             // digest of the staged baseline
    std::string created_at;
};

// Creates a controller-owned staged copy of the authorized subset of the root (or a Git
// worktree when the root is a compatible Git repository and Git is available). Never runs
// `git init`; never writes the user's root.
Expected<StagedWorkspace> CreateStaging(const std::string& project_id, const std::string& task_id,
                                        const std::string& canonical_root,
                                        const std::vector<std::string>& allowed_relative_paths,
                                        const std::string& workspace_root);

// Computes the change set of a staged workspace versus its recorded baseline.
struct ChangeSetEntry {
    std::string rel_path;
    std::string operation;                   // CREATE | MODIFY | DELETE
    std::string baseline_sha256;
    std::string result_sha256;
    std::uint64_t size = 0;
};
Expected<std::vector<ChangeSetEntry>> ComputeChangeSet(const StagedWorkspace& staged);

// --- Integration candidate ---------------------------------------------------------------

struct IntegrationCandidate {
    std::string candidate_id;
    std::string project_id;
    std::string task_id;
    std::vector<ChangeSetEntry> changes;
    std::vector<std::string> conflicts;      // files where the current root diverged from baseline
    std::string created_at;
};

// Builds the integration candidate and checks it against the *current* root contents:
// a file that changed in the root since the baseline is a conflict, not something to
// overwrite. Fails closed on unreadable/aliased paths.
Expected<IntegrationCandidate> BuildIntegrationCandidate(const StagedWorkspace& staged,
                                                         const std::string& canonical_root);

// --- Publication journal and guarded publication -----------------------------------------

struct PublicationPlan {
    std::string publication_id;
    std::string project_id;
    std::string task_id;
    std::vector<ChangeSetEntry> changes;
    std::string created_at;
};

struct PublicationOutcome {
    std::string publication_id;
    std::string state;                       // PUBLISHED | CONFLICT | FAILED | RECOVERING
    std::vector<std::string> applied_paths;
    std::vector<std::string> conflicted_paths;
    std::vector<std::string> failed_paths;
    std::string detail;
};

// Persists the durable journal (per-file before/after digests, staged paths, recovery
// material) before any mutation, then applies guarded same-volume operations: revalidate the
// current root file hash immediately before each mutation; write a same-volume temp file and
// replace; never blindly overwrite a concurrently edited user file.
Expected<PublicationOutcome> Publish(const PublicationPlan& plan, const std::string& canonical_root,
                                     storage::Store* store,
                                     const std::string& recovery_root);

// Reconciliation after restart or partial failure: compares the journal with actual hashes
// and reports the truthful state per file without rolling back newer user edits.
Expected<PublicationOutcome> Reconcile(const std::string& publication_id, storage::Store* store,
                                       const std::string& canonical_root);

// Lists publications that are not in a terminal state (for startup reconciliation).
Expected<std::vector<nlohmann::json>> PendingPublications(storage::Store* store);

// --- Path safety helpers -----------------------------------------------------------------

// True when the relative path is safe: no absolute prefix, no drive letter, no '..' escape,
// no alternate data stream, no reserved device name.
bool IsSafeRelativePath(const std::string& relative_path);

}  // namespace mayasaba::workspace
