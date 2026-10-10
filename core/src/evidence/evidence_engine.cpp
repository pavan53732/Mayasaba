// Evidence Engine (layer 11) implementation: collection with immediate hash verification,
// persisted provenance, criterion aggregation. Missing or mismatched bytes are integrity
// failures, not valid evidence.
#include "mayasaba/evidence.hpp"

#include <algorithm>
#include <set>

#include <windows.h>
#include <winternl.h>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::evidence {
namespace {

const char* kOutcomeNames[] = {"PASS", "FAIL", "INCONCLUSIVE", "BLOCKED", "NOT_APPLICABLE"};

nlohmann::json ArtifactToJson(const ArtifactClaim& artifact) {
    return {{"path", artifact.path},
            {"sha256", artifact.sha256},
            {"size", artifact.size},
            {"kind", artifact.kind},
            {"provenance", artifact.provenance},
            {"collected_at", artifact.collected_at},
            {"verified", artifact.verified},
            {"exists", artifact.exists}};
}

ArtifactClaim ArtifactFromJson(const nlohmann::json& value) {
    ArtifactClaim artifact;
    artifact.path = value.value("path", std::string());
    artifact.sha256 = value.value("sha256", std::string());
    artifact.size = value.value("size", std::uint64_t{0});
    artifact.kind = value.value("kind", std::string());
    artifact.provenance = value.value("provenance", std::string());
    artifact.collected_at = value.value("collected_at", std::string());
    artifact.verified = value.value("verified", false);
    artifact.exists = value.value("exists", false);
    return artifact;
}

nlohmann::json CitationToJson(const Citation& citation) {
    return {{"source", citation.source},       {"span", citation.span},
            {"text", citation.text},           {"sha256", citation.sha256},
            {"provenance", citation.provenance}, {"collected_at", citation.collected_at}};
}

Citation CitationFromJson(const nlohmann::json& value) {
    Citation citation;
    citation.source = value.value("source", std::string());
    citation.span = value.value("span", std::string());
    citation.text = value.value("text", std::string());
    citation.sha256 = value.value("sha256", std::string());
    citation.provenance = value.value("provenance", std::string());
    citation.collected_at = value.value("collected_at", std::string());
    return citation;
}

nlohmann::json EnvironmentToJson(const Environment& environment) {
    return {{"host", environment.host},
            {"user", environment.user},
            {"cwd", environment.cwd},
            {"arch", environment.arch},
            {"os_version", environment.os_version},
            {"toolchain", environment.toolchain},
            {"path_variables_omitted", environment.path_variables_omitted}};
}

Environment EnvironmentFromJson(const nlohmann::json& value) {
    Environment environment;
    environment.host = value.value("host", std::string());
    environment.user = value.value("user", std::string());
    environment.cwd = value.value("cwd", std::string());
    environment.arch = value.value("arch", std::string());
    environment.os_version = value.value("os_version", std::string());
    environment.toolchain = value.value("toolchain", std::string());
    if (value.contains("path_variables_omitted") && value["path_variables_omitted"].is_array()) {
        for (const auto& item : value["path_variables_omitted"]) {
            if (item.is_string()) environment.path_variables_omitted.push_back(item.get<std::string>());
        }
    }
    return environment;
}

}  // namespace

std::string Engine::OutcomeName(const std::string& raw) {
    std::string upper;
    upper.reserve(raw.size());
    for (char ch : raw) upper.push_back(static_cast<char>(::toupper(static_cast<unsigned char>(ch))));
    for (const char* name : kOutcomeNames) {
        if (upper == name) return name;
    }
    return {};
}

Expected<Environment> Engine::CaptureEnvironment() {
    Environment environment;

    wchar_t buffer[512];
    DWORD size = 512;
    if (GetComputerNameW(buffer, &size)) {
        auto utf8 = WideToUtf8(std::wstring(buffer, size));
        if (utf8.ok()) environment.host = utf8.value();
    }
    size = 512;
    if (GetUserNameW(buffer, &size)) {
        auto utf8 = WideToUtf8(std::wstring(buffer, size > 0 ? size - 1 : 0));
        if (utf8.ok()) environment.user = utf8.value();
    }
    size = GetCurrentDirectoryW(512, buffer);
    if (size > 0 && size < 512) {
        auto utf8 = WideToUtf8(std::wstring(buffer, size));
        if (utf8.ok()) environment.cwd = utf8.value();
    }

    SYSTEM_INFO info{};
    GetNativeSystemInfo(&info);
    switch (info.wProcessorArchitecture) {
        case PROCESSOR_ARCHITECTURE_AMD64: environment.arch = "x64"; break;
        case PROCESSOR_ARCHITECTURE_ARM64: environment.arch = "arm64"; break;
        case PROCESSOR_ARCHITECTURE_INTEL: environment.arch = "x86"; break;
        default: environment.arch = "unknown"; break;
    }

    // RtlGetVersion reports the true OS build regardless of manifest shims.
    using RtlGetVersionFn = LONG(WINAPI*)(PRTL_OSVERSIONINFOW);
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    if (ntdll) {
        auto fn = reinterpret_cast<RtlGetVersionFn>(
            reinterpret_cast<void*>(GetProcAddress(ntdll, "RtlGetVersion")));
        if (fn) {
            RTL_OSVERSIONINFOW version{};
            version.dwOSVersionInfoSize = sizeof(version);
            if (fn(&version) == 0) {
                environment.os_version = "Windows " + std::to_string(version.dwMajorVersion) + "." +
                                         std::to_string(version.dwMinorVersion) + " build " +
                                         std::to_string(version.dwBuildNumber);
            }
        }
    }

    const char* toolchain = std::getenv("MAYASABA_TOOLCHAIN");
    environment.toolchain = toolchain ? toolchain : "unknown";
    environment.path_variables_omitted = {"PATH", "PATHEXT"};
    return environment;
}

Expected<EvidenceRecord> Engine::Collect(const EvidenceRecord& draft) {
    if (draft.project_id.empty() || draft.criterion_id.empty() || draft.check_id.empty()) {
        return Fail<EvidenceRecord>(ErrorCode::InvalidArgument,
                                    "evidence requires project_id, criterion_id and check_id");
    }
    const std::string outcome = OutcomeName(draft.outcome);
    if (outcome.empty()) {
        return Fail<EvidenceRecord>(ErrorCode::SchemaViolation,
                                    "evidence outcome must be PASS/FAIL/INCONCLUSIVE/BLOCKED/"
                                    "NOT_APPLICABLE, got: " + draft.outcome);
    }

    EvidenceRecord record = draft;
    record.outcome = outcome;
    if (record.evidence_id.empty()) record.evidence_id = NewId("evi");
    if (record.collected_at.empty()) record.collected_at = NowUtcIso8601();
    if (record.artifact.collected_at.empty()) record.artifact.collected_at = record.collected_at;
    if (record.environment.host.empty()) {
        auto environment = CaptureEnvironment();
        if (environment.ok()) record.environment = environment.value();
    }

    // Artifact verification happens at collection time. A claimed hash that does not match the
    // bytes on disk (or missing bytes) is an integrity failure, never a valid record.
    if (record.artifact.kind == "FILE") {
        if (record.artifact.path.empty()) {
            return Fail<EvidenceRecord>(ErrorCode::InvalidArgument,
                                        "FILE evidence requires an artifact path");
        }
        auto identity = fs::IdentifyPath(record.artifact.path);
        if (!identity.ok() || !identity.value().exists || identity.value().is_directory) {
            return Fail<EvidenceRecord>(ErrorCode::IntegrityFailure,
                                        "artifact missing or not a file: " +
                                            record.artifact.path);
        }
        auto digest = Sha256::HexOfFile(record.artifact.path);
        if (!digest.ok()) {
            return Fail<EvidenceRecord>(ErrorCode::IntegrityFailure,
                                        "artifact unreadable: " + digest.message());
        }
        if (!record.artifact.sha256.empty() && record.artifact.sha256 != digest.value()) {
            return Fail<EvidenceRecord>(
                ErrorCode::IntegrityFailure,
                "artifact hash mismatch for " + record.artifact.path + ": claimed " +
                    record.artifact.sha256 + ", observed " + digest.value());
        }
        record.artifact.sha256 = digest.value();
        record.artifact.size = identity.value().size;
        record.artifact.exists = true;
        record.artifact.verified = true;
    } else if (record.artifact.kind == "DIRECTORY") {
        auto identity = fs::IdentifyPath(record.artifact.path);
        record.artifact.exists = identity.ok() && identity.value().exists &&
                                  identity.value().is_directory;
        if (!record.artifact.exists) {
            return Fail<EvidenceRecord>(ErrorCode::IntegrityFailure,
                                        "directory missing: " + record.artifact.path);
        }
    } else {
        // Captured text/stream evidence: existence is inherent in the record itself; a hash is
        // recorded only when the collector supplied one.
        record.artifact.exists = true;
        record.artifact.verified = !record.artifact.sha256.empty();
    }

    auto payload = nlohmann::json{{"evidence_id", record.evidence_id},
                                  {"criterion_id", record.criterion_id},
                                  {"check_id", record.check_id},
                                  {"outcome", record.outcome}};
    std::string artifact_json = ArtifactToJson(record.artifact).dump();
    std::string citation_json = CitationToJson(record.citation).dump();
    std::string environment_json = EnvironmentToJson(record.environment).dump();

    auto store_status = store_->Exec(
        "CREATE TABLE IF NOT EXISTS evidence_records("
        "evidence_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, criterion_id TEXT NOT NULL, "
        "check_id TEXT NOT NULL, check_version TEXT NOT NULL, outcome TEXT NOT NULL, "
        "detail TEXT NOT NULL, collector TEXT NOT NULL, collected_at TEXT NOT NULL, "
        "artifact TEXT NOT NULL, citation TEXT NOT NULL, environment TEXT NOT NULL);"
        "CREATE INDEX IF NOT EXISTS idx_evidence_criterion ON evidence_records(criterion_id);");
    if (!store_status.ok()) return Expected<EvidenceRecord>(store_status);

    auto inserted = store_->Exec(
        "INSERT INTO evidence_records(evidence_id, project_id, criterion_id, check_id, "
        "check_version, outcome, detail, collector, collected_at, artifact, citation, environment) "
        "VALUES(?,?,?,?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(record.evidence_id), storage::SqlValue::Text(record.project_id),
         storage::SqlValue::Text(record.criterion_id), storage::SqlValue::Text(record.check_id),
         storage::SqlValue::Text(record.check_version), storage::SqlValue::Text(record.outcome),
         storage::SqlValue::Text(RedactSecrets(record.detail)),
         storage::SqlValue::Text(record.collector), storage::SqlValue::Text(record.collected_at),
         storage::SqlValue::Text(artifact_json), storage::SqlValue::Text(citation_json),
         storage::SqlValue::Text(environment_json)});
    if (!inserted.ok()) return Expected<EvidenceRecord>(inserted);

    auto event = store_->AppendEvent(record.project_id, "evidence.recorded", payload);
    if (!event.ok()) return Expected<EvidenceRecord>(event.status());
    return record;
}

Expected<std::vector<EvidenceRecord>> Engine::Records(const std::string& criterion_id) {
    auto rows = store_->Query(
        "SELECT evidence_id, project_id, criterion_id, check_id, check_version, outcome, detail, "
        "collector, collected_at, artifact, citation, environment FROM evidence_records "
        "WHERE criterion_id=? ORDER BY collected_at ASC, evidence_id ASC;",
        {storage::SqlValue::Text(criterion_id)});
    if (!rows.ok()) return Expected<std::vector<EvidenceRecord>>(rows.status());
    std::vector<EvidenceRecord> out;
    out.reserve(rows.value().size());
    for (const auto& row : rows.value()) {
        EvidenceRecord record;
        record.evidence_id = row.Text("evidence_id");
        record.project_id = row.Text("project_id");
        record.criterion_id = row.Text("criterion_id");
        record.check_id = row.Text("check_id");
        record.check_version = row.Text("check_version");
        record.outcome = row.Text("outcome");
        record.detail = row.Text("detail");
        record.collector = row.Text("collector");
        record.collected_at = row.Text("collected_at");
        auto artifact = ParseJsonBounded(row.Text("artifact"));
        if (artifact.ok()) record.artifact = ArtifactFromJson(artifact.value());
        auto citation = ParseJsonBounded(row.Text("citation"));
        if (citation.ok()) record.citation = CitationFromJson(citation.value());
        auto environment = ParseJsonBounded(row.Text("environment"));
        if (environment.ok()) record.environment = EnvironmentFromJson(environment.value());
        out.push_back(std::move(record));
    }
    return out;
}

Expected<Engine::CriterionOutcome> Engine::Decide(const std::string& criterion_id) {
    auto records = Records(criterion_id);
    if (!records.ok()) return Expected<CriterionOutcome>(records.status());

    CriterionOutcome outcome;
    outcome.criterion_id = criterion_id;
    outcome.records = records.value();

    if (records.value().empty()) {
        outcome.outcome = "INCONCLUSIVE";
        outcome.rationale = "no evidence collected for criterion " + criterion_id;
        outcome.binding = true;
        return outcome;
    }

    bool any_fail = false, any_blocked = false, any_pass = false, any_inconclusive = false,
         any_na = false;
    std::vector<std::string> fail_refs;
    for (const auto& record : records.value()) {
        if (record.outcome == "FAIL") {
            any_fail = true;
            fail_refs.push_back(record.evidence_id);
        } else if (record.outcome == "BLOCKED") {
            any_blocked = true;
        } else if (record.outcome == "PASS") {
            any_pass = true;
        } else if (record.outcome == "INCONCLUSIVE") {
            any_inconclusive = true;
        } else if (record.outcome == "NOT_APPLICABLE") {
            any_na = true;
        }
    }

    // A blocking FAIL, INCONCLUSIVE or BLOCKED can never be hidden by other passes.
    if (any_fail) {
        outcome.outcome = "FAIL";
        outcome.rationale = "failing evidence recorded (" + std::to_string(fail_refs.size()) +
                            " record(s)); a failure cannot be averaged away";
    } else if (any_blocked) {
        outcome.outcome = "BLOCKED";
        outcome.rationale = "required proof or capability missing; the gate fails closed";
    } else if (any_inconclusive) {
        outcome.outcome = "INCONCLUSIVE";
        outcome.rationale = "inconclusive evidence present; the criterion is not satisfied";
    } else if (any_pass) {
        outcome.outcome = "PASS";
        outcome.rationale = "all collected evidence passes";
    } else if (any_na) {
        outcome.outcome = "NOT_APPLICABLE";
        outcome.rationale = "criterion declared not applicable with recorded justification";
    } else {
        outcome.outcome = "INCONCLUSIVE";
        outcome.rationale = "no decisive evidence";
    }
    outcome.binding = !(outcome.outcome == "PASS" || outcome.outcome == "NOT_APPLICABLE");
    return outcome;
}

}  // namespace mayasaba::evidence
