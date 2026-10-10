// Validation / Repair Engine (layer 10): validation verdicts, diagnostics, repair dispositions.
// Separated from the Task engine so verdicts are recomputable from evidence alone and repairs
// do not silently erase history. Every verdict is bound to a check identity/version and an
// evidence hash.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/evidence.hpp"
#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/tasks.hpp"

namespace mayasaba::validation {

enum class Verdict { Pass, Fail, Inconclusive, Blocked, NotApplicable };
const char* VerdictName(Verdict verdict);
std::optional<Verdict> ParseVerdict(const std::string& name);

struct Diagnostic {
    std::string code;                     // stable error/diagnostic code
    std::string message;
    std::string path;                     // canonical where applicable
    std::string span;                     // line/byte range
    nlohmann::json subject = nlohmann::json::object();
};

enum class FailureKind {
    CodeOrTest,
    IntegrationConflict,
    StaleContext,
    DependencyOrToolchain,
    CliOrProvider,
    PolicyOrScope,
    ExternalEnvironment,
    Unknown,
};
const char* FailureKindName(FailureKind kind);

struct ValidationResult {
    std::string check_id;
    std::string check_version;
    std::string criterion_id;
    Verdict verdict = Verdict::Inconclusive;
    std::string rationale;               // why this verdict, with evidence ref
    std::vector<Diagnostic> diagnostics;
    FailureKind failure_kind = FailureKind::Unknown;  // when verdict is Fail
    std::string evidence_hash;           // MCB-1 digest over the supporting evidence records
    std::string validated_at;
};

struct RepairDisposition {
    std::string repair_id;
    std::string validation_id;
    std::string hypothesis;              // falsifiable root cause
    std::string planned_changes;         // bounded, specific
    std::string outcome;                 // ATTEMPTED | SUCCEEDED | FAILED | BLOCKED
    std::vector<Diagnostic> diagnostics; // post-repair diagnostics
    std::string evidence_hash;
    std::string repaired_at;
    bool supersedes_history = false;     // never silently rewrite prior history
};

class Engine {
public:
    explicit Engine(storage::Store* store, evidence::Engine* evidence)
        : store_(store), evidence_(evidence) {}

    // Verifies a criterion's oracle adequacy and recomputes the verdict from accumulated
    // evidence. Oracle adequacy: the check must actually discriminate the requested property.
    Expected<ValidationResult> Validate(const tasks::AcceptanceCriterion& criterion,
                                        const std::vector<evidence::EvidenceRecord>& evidence_records);

    // Records an attempted repair with its hypothesis and bounded changes.
    Expected<RepairDisposition> RecordRepair(const RepairDisposition& draft);

private:
    storage::Store* store_;
    evidence::Engine* evidence_;
};

}  // namespace mayasaba::validation
