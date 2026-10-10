// Validation / Repair Engine (layer 10) implementation: verdicts recomputed from evidence with
// oracle-adequacy checks, diagnostics, and bounded repair dispositions. History is append-only.
#include "mayasaba/validation.hpp"

#include <set>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"

namespace mayasaba::validation {
namespace {

const char* kVerdictNames[] = {"PASS", "FAIL", "INCONCLUSIVE", "BLOCKED", "NOT_APPLICABLE"};

const std::set<std::string>& KnownOracleClasses() {
    static const std::set<std::string> classes = {
        "policy",   "artifact_integrity", "static",       "compile",        "unit",
        "integration", "runtime",         "e2e",          "data_integrity", "citation",
        "user_acceptance",
    };
    return classes;
}

// Oracle classes whose verdict requires a declared, executable check procedure.
const std::set<std::string>& CheckRequiredClasses() {
    static const std::set<std::string> classes = {
        "static", "compile", "unit", "integration", "runtime", "e2e", "data_integrity",
    };
    return classes;
}

nlohmann::json DiagnosticsToJson(const std::vector<Diagnostic>& diagnostics) {
    nlohmann::json out = nlohmann::json::array();
    for (const auto& diagnostic : diagnostics) {
        out.push_back({{"code", diagnostic.code},
                       {"message", diagnostic.message},
                       {"path", diagnostic.path},
                       {"span", diagnostic.span},
                       {"subject", diagnostic.subject}});
    }
    return out;
}

std::vector<Diagnostic> DiagnosticsFromJson(const nlohmann::json& value) {
    std::vector<Diagnostic> out;
    if (!value.is_array()) return out;
    for (const auto& item : value) {
        Diagnostic diagnostic;
        diagnostic.code = item.value("code", std::string());
        diagnostic.message = item.value("message", std::string());
        diagnostic.path = item.value("path", std::string());
        diagnostic.span = item.value("span", std::string());
        if (item.contains("subject")) diagnostic.subject = item["subject"];
        out.push_back(std::move(diagnostic));
    }
    return out;
}

Status EnsureSchema(storage::Store* store) {
    auto s1 = store->Exec(
        "CREATE TABLE IF NOT EXISTS validation_results("
        "row_id INTEGER PRIMARY KEY AUTOINCREMENT, check_id TEXT NOT NULL, "
        "check_version TEXT NOT NULL, criterion_id TEXT NOT NULL, verdict TEXT NOT NULL, "
        "rationale TEXT NOT NULL, diagnostics TEXT NOT NULL, failure_kind TEXT NOT NULL, "
        "evidence_hash TEXT NOT NULL, validated_at TEXT NOT NULL);");
    if (!s1.ok()) return s1;
    auto s2 = store->Exec(
        "CREATE INDEX IF NOT EXISTS idx_validation_criterion ON validation_results(criterion_id);");
    if (!s2.ok()) return s2;
    return store->Exec(
        "CREATE TABLE IF NOT EXISTS repairs("
        "repair_id TEXT PRIMARY KEY, validation_id TEXT NOT NULL, hypothesis TEXT NOT NULL, "
        "planned_changes TEXT NOT NULL, outcome TEXT NOT NULL, diagnostics TEXT NOT NULL, "
        "evidence_hash TEXT NOT NULL, repaired_at TEXT NOT NULL, supersedes_history INTEGER NOT NULL);");
}

}  // namespace

const char* VerdictName(Verdict verdict) {
    switch (verdict) {
        case Verdict::Pass: return "PASS";
        case Verdict::Fail: return "FAIL";
        case Verdict::Inconclusive: return "INCONCLUSIVE";
        case Verdict::Blocked: return "BLOCKED";
        case Verdict::NotApplicable: return "NOT_APPLICABLE";
    }
    return "INCONCLUSIVE";
}

std::optional<Verdict> ParseVerdict(const std::string& name) {
    for (const char* candidate : kVerdictNames) {
        if (name == candidate) {
            if (name == std::string("PASS")) return Verdict::Pass;
            if (name == std::string("FAIL")) return Verdict::Fail;
            if (name == std::string("INCONCLUSIVE")) return Verdict::Inconclusive;
            if (name == std::string("BLOCKED")) return Verdict::Blocked;
            return Verdict::NotApplicable;
        }
    }
    return std::nullopt;
}

const char* FailureKindName(FailureKind kind) {
    switch (kind) {
        case FailureKind::CodeOrTest: return "CODE_OR_TEST";
        case FailureKind::IntegrationConflict: return "INTEGRATION_CONFLICT";
        case FailureKind::StaleContext: return "STALE_CONTEXT";
        case FailureKind::DependencyOrToolchain: return "DEPENDENCY_OR_TOOLCHAIN";
        case FailureKind::CliOrProvider: return "CLI_OR_PROVIDER";
        case FailureKind::PolicyOrScope: return "POLICY_OR_SCOPE";
        case FailureKind::ExternalEnvironment: return "EXTERNAL_ENVIRONMENT";
        case FailureKind::Unknown: return "UNKNOWN";
    }
    return "UNKNOWN";
}

Expected<ValidationResult> Engine::Validate(
    const tasks::AcceptanceCriterion& criterion,
    const std::vector<evidence::EvidenceRecord>& evidence_records) {
    if (criterion.criterion_id.empty()) {
        return Fail<ValidationResult>(ErrorCode::InvalidArgument, "criterion has no id");
    }

    ValidationResult result;
    result.criterion_id = criterion.criterion_id;
    result.validated_at = NowUtcIso8601();
    if (result.check_id.empty()) result.check_id = "check:" + criterion.criterion_id;
    result.check_version = "1";

    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<ValidationResult>(schema);

    // Oracle adequacy: the oracle must actually discriminate the requested property. An
    // expectation and a known oracle class are mandatory; procedure-driven classes also need a
    // declared check specification.
    std::string adequacy_problem;
    if (criterion.expectation.empty()) {
        adequacy_problem = "criterion has no observable expectation";
    } else if (criterion.oracle_class.empty()) {
        adequacy_problem = "no oracle class declared";
    } else if (KnownOracleClasses().find(criterion.oracle_class) == KnownOracleClasses().end()) {
        adequacy_problem = "unknown oracle class: " + criterion.oracle_class;
    } else if (CheckRequiredClasses().find(criterion.oracle_class) != CheckRequiredClasses().end() &&
               criterion.oracle_spec.empty()) {
        adequacy_problem = "oracle inadequate: class '" + criterion.oracle_class +
                           "' requires a declared check specification";
    }

    // Select the evidence bound to this criterion.
    std::vector<const evidence::EvidenceRecord*> relevant;
    for (const auto& record : evidence_records) {
        if (record.criterion_id == criterion.criterion_id) relevant.push_back(&record);
    }

    // Digest over the supporting evidence (identity + outcome + artifact hash).
    nlohmann::json digest_input = nlohmann::json::array();
    for (const auto* record : relevant) {
        digest_input.push_back({{"evidence_id", record->evidence_id},
                                {"outcome", record->outcome},
                                {"artifact_sha256", record->artifact.sha256},
                                {"check_id", record->check_id},
                                {"check_version", record->check_version}});
    }
    auto digest = CanonicalDigest(digest_input);
    result.evidence_hash = digest.ok() ? digest.value() : std::string();

    bool any_fail = false, any_blocked = false, any_pass = false, any_inconclusive = false,
         any_na = false;
    for (const auto* record : relevant) {
        if (record->outcome == "FAIL") any_fail = true;
        else if (record->outcome == "BLOCKED") any_blocked = true;
        else if (record->outcome == "PASS") any_pass = true;
        else if (record->outcome == "INCONCLUSIVE") any_inconclusive = true;
        else if (record->outcome == "NOT_APPLICABLE") any_na = true;
    }

    if (relevant.empty()) {
        result.verdict = Verdict::Inconclusive;
        result.rationale = "no evidence collected for criterion " + criterion.criterion_id;
    } else if (any_fail) {
        result.verdict = Verdict::Fail;
        result.rationale = "failing evidence recorded; a blocking FAIL cannot be hidden";
        result.failure_kind = FailureKind::Unknown;
        result.diagnostics.push_back(
            {"E_VALIDATION_FAILED", "criterion has failing evidence", "", "", nlohmann::json::object()});
    } else if (any_blocked) {
        result.verdict = Verdict::Blocked;
        result.rationale = "required proof or capability missing; failing closed";
    } else if (!adequacy_problem.empty()) {
        // A pass can only be granted by an adequate oracle.
        result.verdict = Verdict::Inconclusive;
        result.rationale = adequacy_problem;
    } else if (any_inconclusive) {
        result.verdict = Verdict::Inconclusive;
        result.rationale = "inconclusive evidence present; criterion not satisfied";
    } else if (any_pass) {
        result.verdict = Verdict::Pass;
        result.rationale = "all collected evidence passes and the oracle is adequate";
    } else if (any_na) {
        result.verdict = Verdict::NotApplicable;
        result.rationale = "criterion declared not applicable with recorded justification";
    } else {
        result.verdict = Verdict::Inconclusive;
        result.rationale = "no decisive evidence";
    }

    auto inserted = store_->Exec(
        "INSERT INTO validation_results(check_id, check_version, criterion_id, verdict, "
        "rationale, diagnostics, failure_kind, evidence_hash, validated_at) "
        "VALUES(?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(result.check_id), storage::SqlValue::Text(result.check_version),
         storage::SqlValue::Text(result.criterion_id),
         storage::SqlValue::Text(VerdictName(result.verdict)),
         storage::SqlValue::Text(result.rationale),
         storage::SqlValue::Text(DiagnosticsToJson(result.diagnostics).dump()),
         storage::SqlValue::Text(FailureKindName(result.failure_kind)),
         storage::SqlValue::Text(result.evidence_hash),
         storage::SqlValue::Text(result.validated_at)});
    if (!inserted.ok()) return Expected<ValidationResult>(inserted);

    auto event = store_->AppendEvent(
        "", "validation.result.recorded",
        {{"criterion_id", result.criterion_id},
         {"verdict", VerdictName(result.verdict)},
         {"evidence_hash", result.evidence_hash}});
    if (!event.ok()) return Expected<ValidationResult>(event.status());
    return result;
}

Expected<RepairDisposition> Engine::RecordRepair(const RepairDisposition& draft) {
    if (draft.validation_id.empty() || draft.hypothesis.empty()) {
        return Fail<RepairDisposition>(ErrorCode::InvalidArgument,
                                       "repair requires a validation id and a falsifiable "
                                       "hypothesis");
    }
    static const std::set<std::string> kOutcomes = {"ATTEMPTED", "SUCCEEDED", "FAILED", "BLOCKED"};
    if (kOutcomes.find(draft.outcome) == kOutcomes.end()) {
        return Fail<RepairDisposition>(ErrorCode::InvalidArgument,
                                       "repair outcome must be ATTEMPTED/SUCCEEDED/FAILED/BLOCKED");
    }
    auto schema = EnsureSchema(store_);
    if (!schema.ok()) return Expected<RepairDisposition>(schema);

    auto existing = store_->Query("SELECT row_id FROM validation_results WHERE check_id=?;",
                                  {storage::SqlValue::Text(draft.validation_id)});
    if (!existing.ok()) return Expected<RepairDisposition>(existing.status());
    if (existing.value().empty()) {
        return Fail<RepairDisposition>(ErrorCode::NotFound,
                                       "no validation result for id: " + draft.validation_id);
    }

    RepairDisposition repair = draft;
    if (repair.repair_id.empty()) repair.repair_id = NewId("rep");
    repair.repaired_at = NowUtcIso8601();
    if (repair.evidence_hash.empty()) {
        auto digest = CanonicalDigest(nlohmann::json{{"repair_id", repair.repair_id},
                                                     {"validation_id", repair.validation_id},
                                                     {"outcome", repair.outcome}});
        if (digest.ok()) repair.evidence_hash = digest.value();
    }
    auto inserted = store_->Exec(
        "INSERT INTO repairs(repair_id, validation_id, hypothesis, planned_changes, outcome, "
        "diagnostics, evidence_hash, repaired_at, supersedes_history) VALUES(?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(repair.repair_id),
         storage::SqlValue::Text(repair.validation_id),
         storage::SqlValue::Text(repair.hypothesis),
         storage::SqlValue::Text(repair.planned_changes),
         storage::SqlValue::Text(repair.outcome),
         storage::SqlValue::Text(DiagnosticsToJson(repair.diagnostics).dump()),
         storage::SqlValue::Text(repair.evidence_hash),
         storage::SqlValue::Text(repair.repaired_at),
         storage::SqlValue::Int(repair.supersedes_history ? 1 : 0)});
    if (!inserted.ok()) return Expected<RepairDisposition>(inserted);
    return repair;
}

}  // namespace mayasaba::validation
