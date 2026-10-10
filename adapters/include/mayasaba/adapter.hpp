// Agent Gateway / Adapters (layer 4).
//
// One adapter per CLI. Each adapter is the only place that CLI's native protocol exists;
// adapters normalize to typed events and never alter the user's model/provider configuration.
#pragma once

#include <functional>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"

namespace mayasaba::adapters {

// Exactly three agents, no others (product invariant).
enum class AgentKind { Hermes = 0, Kilo = 1, Claude = 2 };

constexpr int kAgentCount = 3;
const char* AgentName(AgentKind kind);          // "Hermes" | "Kilo Code" | "Claude Code"
const char* AgentId(AgentKind kind);            // "hermes" | "kilo" | "claude"
std::optional<AgentKind> ParseAgentId(const std::string& id);
std::vector<AgentKind> AllAgents();

// Adapter readiness labels (typed controller-query projection; NOT an Agent Session state).
enum class Readiness { Checking, Ready, Missing, Unsupported, ProbeFailed };
const char* ReadinessName(Readiness readiness);

struct AdapterStatus {
    AgentKind agent = AgentKind::Hermes;
    Readiness readiness = Readiness::Checking;
    std::string reason;                    // exact observed reason for non-READY states
    std::string executable_path;           // discovered executable (may be empty)
    std::string executable_sha256;         // identity hash of the discovered executable
    std::string probed_at;
    nlohmann::json observed_capabilities = nlohmann::json::object();

    bool needs_attention() const {
        return readiness == Readiness::Missing || readiness == Readiness::Unsupported ||
               readiness == Readiness::ProbeFailed;
    }
};

// Executable discovery: explicit configured path first, then PATH, then known install
// locations. Discovery never executes the CLI and never inspects release numbers.
struct DiscoveryResult {
    std::string path;                      // empty when not found
    std::vector<std::string> searched;     // candidates considered, for diagnostics
};
DiscoveryResult DiscoverExecutable(AgentKind kind, const std::string& explicit_path = {});

// A session request declares everything the adapter needs; it never carries model/provider
// selection. project_root and execution_working_directory are canonical absolute paths.
struct SessionSpec {
    std::string project_id;
    std::string project_root;
    std::string execution_working_directory;
    std::string workspace_view_id;
    std::string task_id;
    std::string attempt_id;
    std::string lease_id;
    std::int64_t lease_version = 0;
    std::int64_t project_epoch = 0;
    std::vector<std::string> allowed_relative_paths;   // read scope for the session
    std::vector<std::string> allowed_write_paths;      // write scope (empty for read-only)
    bool read_only = true;
    bool allow_native_web_tools = false;               // assignment-scoped research permission
    std::string prompt;                                // task prompt (never contains secrets)
};

// One normalized event from a CLI's native stream.
struct SessionEvent {
    std::string kind;        // "started" | "message" | "tool" | "usage" | "error" | "done"
    std::string text;        // message/error text (already redacted)
    nlohmann::json payload = nlohmann::json::object();
};

// The typed adapter surface. Implemented by CliAdapter (one instance per CLI).
class Adapter {
public:
    virtual ~Adapter() = default;
    virtual AgentKind kind() const = 0;

    // Bounded, non-mutating readiness check. Must not consume model usage and must not run
    // release-number commands.
    virtual Expected<AdapterStatus> Probe() = 0;

    // Starts a supervised headless session. Fails closed when the required behavior or
    // containment cannot be established.
    virtual Status StartSession(const SessionSpec& spec, const std::string& session_id) = 0;

    // Sends a follow-up prompt to a live session.
    virtual Status SendPrompt(const std::string& session_id, const std::string& prompt) = 0;

    // Requests cancellation; the observed termination is reported separately.
    virtual Status Cancel(const std::string& session_id, std::string* observed_reason) = 0;

    // Stops and cleans up a session.
    virtual Status Stop(const std::string& session_id) = 0;

    // Drains buffered normalized events for a session.
    virtual std::vector<SessionEvent> DrainEvents(const std::string& session_id) = 0;

    virtual bool HasSession(const std::string& session_id) const = 0;
};

}  // namespace mayasaba::adapters
