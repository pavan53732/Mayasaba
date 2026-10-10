#include "mayasaba/adapter.hpp"

#include <windows.h>

#include <algorithm>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::adapters {

const char* AgentName(AgentKind kind) {
    switch (kind) {
        case AgentKind::Hermes: return "Hermes";
        case AgentKind::Kilo: return "Kilo Code";
        case AgentKind::Claude: return "Claude Code";
    }
    return "Unknown";
}

const char* AgentId(AgentKind kind) {
    switch (kind) {
        case AgentKind::Hermes: return "hermes";
        case AgentKind::Kilo: return "kilo";
        case AgentKind::Claude: return "claude";
    }
    return "unknown";
}

std::optional<AgentKind> ParseAgentId(const std::string& id) {
    if (id == "hermes") return AgentKind::Hermes;
    if (id == "kilo") return AgentKind::Kilo;
    if (id == "claude") return AgentKind::Claude;
    return std::nullopt;
}

std::vector<AgentKind> AllAgents() {
    return {AgentKind::Hermes, AgentKind::Kilo, AgentKind::Claude};
}

const char* ReadinessName(Readiness readiness) {
    switch (readiness) {
        case Readiness::Checking: return "CHECKING";
        case Readiness::Ready: return "READY";
        case Readiness::Missing: return "MISSING";
        case Readiness::Unsupported: return "UNSUPPORTED";
        case Readiness::ProbeFailed: return "PROBE_FAILED";
    }
    return "UNKNOWN";
}

namespace {

std::optional<std::string> SearchOnPath(const std::vector<std::string>& names) {
    wchar_t buffer[32768];
    for (const auto& name : names) {
        std::wstring wide = Utf8ToWide(name);
        if (wide.empty()) continue;
        DWORD length = SearchPathW(nullptr, wide.c_str(), nullptr,
                                   static_cast<DWORD>(std::size(buffer)), buffer, nullptr);
        if (length > 0 && length < std::size(buffer)) {
            auto utf8 = WideToUtf8(std::wstring(buffer, length));
            if (utf8.ok()) return utf8.value();
        }
    }
    return std::nullopt;
}

std::vector<std::string> KnownInstallCandidates(AgentKind kind) {
    std::vector<std::string> candidates;
    wchar_t local_app_data[MAX_PATH * 2] = {};
    DWORD len = GetEnvironmentVariableW(L"LOCALAPPDATA", local_app_data,
                                        static_cast<DWORD>(std::size(local_app_data)));
    std::string lad = len > 0 ? WideToUtf8(std::wstring(local_app_data, len)).value_or("") : "";
    if (!lad.empty()) {
        switch (kind) {
            case AgentKind::Hermes:
                candidates.push_back(fs::JoinPath(lad, "hermes\\hermes-agent\\.hermes\\bin\\hermes.cmd"));
                candidates.push_back(fs::JoinPath(lad, "hermes\\bin\\hermes.exe"));
                candidates.push_back(fs::JoinPath(lad, "Programs\\hermes\\hermes.exe"));
                break;
            case AgentKind::Kilo:
                candidates.push_back(fs::JoinPath(lad, "kilo\\kilo.exe"));
                candidates.push_back(fs::JoinPath(lad, "Programs\\kilo\\kilo.exe"));
                candidates.push_back(fs::JoinPath(lad, "kilo-code\\kilo.exe"));
                break;
            case AgentKind::Claude:
                candidates.push_back(fs::JoinPath(lad, "hermes\\node\\claude.cmd"));
                candidates.push_back(fs::JoinPath(lad, "Programs\\claude\\claude.exe"));
                candidates.push_back(fs::JoinPath(lad, "npm\\claude.cmd"));
                break;
        }
    }
    return candidates;
}

std::vector<std::string> ExecutableNames(AgentKind kind) {
    switch (kind) {
        case AgentKind::Hermes: return {"hermes.exe", "hermes.cmd", "hermes.bat"};
        case AgentKind::Kilo: return {"kilo.exe", "kilo.cmd", "kilo.bat"};
        case AgentKind::Claude: return {"claude.cmd", "claude.exe", "claude.bat", "claude.ps1"};
    }
    return {};
}

}  // namespace

DiscoveryResult DiscoverExecutable(AgentKind kind, const std::string& explicit_path) {
    DiscoveryResult result;
    // 1. Explicit user-configured path (Mayasaba-owned preference; the user points at their
    //    already-installed CLI; Mayasaba never installs or updates it).
    if (!explicit_path.empty()) {
        result.searched.push_back("explicit:" + explicit_path);
        if (fs::PathExists(explicit_path)) {
            result.path = explicit_path;
            return result;
        }
    }
    // 2. PATH lookup of the canonical executable names.
    for (const auto& name : ExecutableNames(kind)) result.searched.push_back("path:" + name);
    if (auto found = SearchOnPath(ExecutableNames(kind))) {
        result.path = *found;
        return result;
    }
    // 3. Known per-user install locations.
    for (const auto& candidate : KnownInstallCandidates(kind)) {
        result.searched.push_back("candidate:" + candidate);
        if (fs::PathExists(candidate)) {
            result.path = candidate;
            return result;
        }
    }
    return result;
}

}  // namespace mayasaba::adapters
