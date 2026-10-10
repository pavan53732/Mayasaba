// Agent Gateway (layer 4 service): owns session identity, health, and adapter-translated
// native traffic for exactly the three installed CLIs. One adapter instance per CLI; the
// gateway routes normalized events onward (MCF-v2 / application services) and keeps the
// observed session state machine (STARTING/RUNNING/EXITED/CANCELLED/FAILED/UNKNOWN).
#pragma once

#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "mayasaba/adapter.hpp"
#include "mayasaba/cli_adapter.hpp"

namespace mayasaba::gateway {

// Observed session state (projection of the Agent Session machine; not a new authority).
enum class SessionState { Starting, Running, Exited, Cancelled, Failed, Unknown };
const char* SessionStateName(SessionState state);

struct SessionRecord {
    std::string session_id;
    adapters::AgentKind agent = adapters::AgentKind::Hermes;
    std::string task_id;
    std::string attempt_id;
    std::string lease_id;
    std::int64_t lease_version = 0;
    std::string project_id;
    SessionState state = SessionState::Starting;
    std::string started_at;
    std::string ended_at;
    std::uint32_t exit_code = 0;
    bool exit_code_observed = false;
    std::size_t event_count = 0;
    std::string detail;                      // last observed failure/cancel reason
};

// Event sink: the gateway hands normalized events plus their session context to the
// application layer. The gateway itself never schedules work and never mutates task records.
using EventSink = std::function<void(const SessionRecord& session,
                                     const std::vector<adapters::SessionEvent>& events)>;

class AgentGateway {
public:
    // `explicit_paths` maps agent id -> user-configured executable path (may be empty).
    explicit AgentGateway(std::map<std::string, std::string> explicit_paths = {});

    // Probes all three adapters (bounded, non-mutating). Returns per-agent status.
    std::map<std::string, adapters::AdapterStatus> ProbeAll();

    // Re-probes a single agent (used by the Chat "Retry" action on a warning card).
    adapters::AdapterStatus Probe(adapters::AgentKind kind);

    adapters::AdapterStatus AgentStatusOf(adapters::AgentKind kind) const;

    // Starts a session for an authorized operation. Fails closed with an exact reason when the
    // agent is not READY, the session spec is invalid, or the profile is unverified.
    Expected<SessionRecord> StartSession(adapters::AgentKind kind,
                                         const adapters::SessionSpec& spec,
                                         const std::string& session_id);

    Status CancelSession(const std::string& session_id, std::string* observed_reason);
    Status StopSession(const std::string& session_id);

    // Drains events for one session and updates its observed state; delivers to the sink.
    Status PumpSession(const std::string& session_id);
    Status PumpAll();

    std::vector<SessionRecord> Sessions() const;
    bool Session(const std::string& session_id, SessionRecord* record) const;

    // Updates the user-configured executable path for one agent (the Settings / "Locate
    // executable" flow). Recreates that agent's adapter so the next probe reflects the
    // change. Never touches model/provider/auth configuration.
    Status SetExecutablePath(adapters::AgentKind kind, const std::string& path);

    void SetEventSink(EventSink sink);

    // Multi-agent operations: FULL council and whole-repository exploration require all three
    // agents; a missing CLI blocks the operation (never reduced to two agents).
    struct AvailabilityReport {
        bool all_ready = false;
        std::vector<std::string> ready_agents;
        std::vector<std::string> unavailable_agents;   // with reasons in `reasons`
        std::map<std::string, std::string> reasons;
    };
    AvailabilityReport Availability() const;

private:
    adapters::CliAdapter* AdapterFor(adapters::AgentKind kind) const;

    std::map<adapters::AgentKind, std::unique_ptr<adapters::CliAdapter>> adapters_;
    mutable std::mutex mutex_;
    std::map<std::string, SessionRecord> sessions_;
    EventSink sink_;
};

}  // namespace mayasaba::gateway
