// Evidence Engine (layer 11): evidence, hashes, citations, provenance. Evidence binds to
// exact artifact hashes, workspace, environment, check identity/version, time and source.
// This is the integrity surface for every acceptance oracle; an agent narrative alone is never
// admissible evidence.
#pragma once

#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::evidence {

struct ArtifactClaim {
    std::string path;                     // canonical absolute path or "memory:<name>"
    std::string sha256;                   // empty when unavailable
    std::uint64_t size = 0;
    std::string kind;                     // FILE | DIRECTORY | STDOUT | STDERR | PROCESS_OUTCOME |
                                         // AGENT_OUTPUT | BUILD_OUTPUT | SNAPSHOT | BLOB
    std::string provenance;               // "agent:<id>@<session>" | "controller" | "user"
    std::string collected_at;
    bool verified = false;                // hash re-checked at collection time
    bool exists = false;
};

struct Citation {
    std::string source;                   // e.g. "build-spec/GLOSSARY@0xabc123"
    std::string span;                     // line range or byte span
    std::string text;                     // excerpt (bounded)
    std::string sha256;                   // of the cited artifact/version
    std::string provenance;
    std::string collected_at;
};

struct Environment {
    std::string host;                     // hostname
    std::string user;                     // sanitized username
    std::string cwd;                      // canonical absolute
    std::string arch;
    std::string os_version;
    std::string toolchain;                // e.g. "MSVC 19.44 / WindowsAppSDK 1.6"
    std::vector<std::string> path_variables_omitted;  // secrets never recorded
};

struct EvidenceRecord {
    std::string evidence_id;
    std::string project_id;
    std::string criterion_id;             // links to a task criterion
    std::string check_id;
    std::string check_version;
    ArtifactClaim artifact;
    Citation citation;
    Environment environment;
    std::string outcome;                  // PASS | FAIL | INCONCLUSIVE | BLOCKED | NOT_APPLICABLE
    std::string detail;
    std::string collected_at;
    std::string collector;                // "controller" | agent id
};

class Engine {
public:
    explicit Engine(storage::Store* store) : store_(store) {}

    // Collects and verifies (where possible) a single evidence record. Verifies file hashes
    // immediately; missing/mismatched bytes are integrity failures, not valid evidence.
    Expected<EvidenceRecord> Collect(const EvidenceRecord& draft);

    Expected<std::vector<EvidenceRecord>> Records(const std::string& criterion_id);

    // Aggregates evidence for a criterion into an outcome with full provenance.
    struct CriterionOutcome {
        std::string criterion_id;
        std::string outcome;              // PASS | FAIL | INCONCLUSIVE | BLOCKED | NOT_APPLICABLE
        std::string rationale;            // why this outcome, with citations
        std::vector<EvidenceRecord> records;
        bool binding = false;             // true when this outcome gates completion
    };
    Expected<CriterionOutcome> Decide(const std::string& criterion_id);

    // Builds the environment snapshot used to bind evidence (never secrets).
    Expected<Environment> CaptureEnvironment();

    static std::string OutcomeName(const std::string& raw);  // normalize PASS/FAIL/etc.

private:
    storage::Store* store_;
};

}  // namespace mayasaba::evidence
