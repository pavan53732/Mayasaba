// Controller facade (layer 2 application boundary for the Control Room UI).
//
// This is the ONLY surface the WinUI layer talks to: typed commands and queries, versioned,
// with plain data results. It deliberately exposes no SQL, no process handles, no scheduling
// internals and no direct agent channel. UI text and agent text are never authority; commands
// are validated and authorized by the controller services behind this boundary.
#pragma once

#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"

// The Win32 API defines SendMessage/SendMessageW as macros in winuser.h. The controller's
// typed command keeps its domain name; neutralize the macro here so call sites are clean.
#ifdef SendMessage
#undef SendMessage
#endif

namespace mayasaba::control {

// Version of the UI<->controller contract (every command/query/result is versioned).
constexpr int kControllerProtocolVersion = 1;

// --- Shared value types -------------------------------------------------------------------

struct AttachmentRef {
    std::string name;
    std::string path;                 // absolute path chosen by the user
    std::uint64_t size = 0;
    std::string sha256;               // computed on accept; empty when unavailable
};

struct CardAction {
    std::string action_id;            // stable id for the card
    std::string label;                // user-visible label
    std::string command;              // command name to invoke (e.g. "RetryAgentProbe")
    nlohmann::json request = nlohmann::json::object();  // typed request payload
};

// One Chat timeline item. The UI renders these; details open inline or in a dismissible sheet.
struct TimelineItem {
    std::uint64_t sequence = 0;
    std::string card_id;
    std::string kind;                 // user_message | agent_message | status | warning |
                                      // question | progress | council_summary |
                                      // requirements_summary | decision | task | validation |
                                      // delivery | error
    std::string title;
    std::string body;
    std::string severity;             // info | attention | error | success
    std::string state;                // operation/task/delivery state where applicable
    bool has_details = false;
    bool dismissible = false;
    std::vector<CardAction> actions;
    nlohmann::json data = nlohmann::json::object();     // typed per-kind payload
    std::string created_at;
};

struct DetailsSection {
    std::string title;
    std::string text;
    std::vector<std::pair<std::string, std::string>> key_values;
    struct Citation {
        std::string source;
        std::string span;
        std::string text;
    };
    std::vector<Citation> citations;
};

struct DetailsPayload {
    std::string card_id;
    std::string title;
    std::vector<DetailsSection> sections;
    nlohmann::json raw = nlohmann::json::object();      // bounded machine-readable backing
};

// --- Commands -----------------------------------------------------------------------------

struct OpenFolderRequest {
    int version = kControllerProtocolVersion;
    std::string display_path;         // path as chosen by the user (folder picker result)
};
struct OpenFolderResult {
    bool ok = false;
    std::string project_id;
    std::string canonical_root;
    std::string display_path;
    std::string detail;               // exact reason when rejected
};

struct SendMessageRequest {
    int version = kControllerProtocolVersion;
    std::string text;
    std::vector<AttachmentRef> attachments;
};
struct SendReceipt {
    std::string contribution_id;      // always set when persisted (even with no CLI available)
    std::string outcome;              // PERSISTED | DENIED | BLOCKED
    std::string detail;
};

struct CancelOperationRequest {
    int version = kControllerProtocolVersion;
    std::string operation_id;         // card_id/operation id from a timeline item
};
struct Ack {
    bool ok = false;
    std::string detail;
};

struct RetryAgentProbeRequest {
    int version = kControllerProtocolVersion;
    std::string agent_id;             // "hermes" | "kilo" | "claude"
};

struct DismissCardRequest {
    int version = kControllerProtocolVersion;
    std::string card_id;
};

struct SetAgentExecutablePathRequest {
    int version = kControllerProtocolVersion;
    std::string agent_id;
    std::string path;                 // user-owned "Locate executable" result (never a model flag)
};

struct RequestDetailsRequest {
    int version = kControllerProtocolVersion;
    std::string card_id;
};

// --- Queries ------------------------------------------------------------------------------

struct ProjectState {
    bool bound = false;               // Send stays disabled until this is true
    std::string project_id;
    std::string canonical_root;
    std::string display_path;
    std::string phase;                // project lifecycle phase (projection)
    std::int64_t epoch = 0;
};

struct AgentStatusView {
    std::string agent_id;             // "hermes" | "kilo" | "claude"
    std::string display_name;         // "Hermes" | "Kilo Code" | "Claude Code"
    std::string readiness;            // CHECKING | READY | MISSING | UNSUPPORTED | PROBE_FAILED
    std::string reason;               // exact observed reason for non-READY states
    bool needs_attention = false;     // UI shows only these in the warning rows
    std::string executable_path;
};

struct TimelineQuery {
    int version = kControllerProtocolVersion;
    std::uint64_t since_sequence = 0; // exclusive; 0 = from the beginning
    std::size_t limit = 200;          // bounded
};
struct TimelinePage {
    std::vector<TimelineItem> items;
    std::uint64_t next_sequence = 0;
    bool more = false;
};

// --- Controller ---------------------------------------------------------------------------

struct ControllerConfig {
    std::string storage_root;         // Mayasaba-owned app data directory (SQLite + blobs)
    std::string workspace_root;       // controller-owned isolated staging root
    std::string recovery_root;        // publication recovery material
    std::map<std::string, std::string> agent_executable_paths;  // agent id -> explicit path
};

class Controller {
public:
    using NoticeCallback = std::function<void()>;   // "something changed"; UI re-queries

    static Expected<std::unique_ptr<Controller>> Create(const ControllerConfig& config);
    virtual ~Controller() = default;

    // Applies migrations, reconciles pending publications, probes the three agents (bounded,
    // non-mutating), and starts background pumping. Must be called once before commands.
    virtual Status Initialize() = 0;
    virtual void Shutdown() = 0;

    // Commands.
    virtual Expected<OpenFolderResult> OpenFolder(const OpenFolderRequest& request) = 0;
    virtual Expected<SendReceipt> SendMessage(const SendMessageRequest& request) = 0;
    virtual Expected<Ack> CancelOperation(const CancelOperationRequest& request) = 0;
    virtual Expected<AgentStatusView> RetryAgentProbe(const RetryAgentProbeRequest& request) = 0;
    virtual Expected<Ack> DismissCard(const DismissCardRequest& request) = 0;
    virtual Expected<Ack> SetAgentExecutablePath(const SetAgentExecutablePathRequest& request) = 0;
    virtual Expected<DetailsPayload> RequestDetails(const RequestDetailsRequest& request) = 0;

    // Queries.
    virtual Expected<ProjectState> QueryProjectState() = 0;
    virtual Expected<TimelinePage> QueryTimeline(const TimelineQuery& query) = 0;
    virtual Expected<std::vector<AgentStatusView>> QueryAgents() = 0;

    // Notification hook: called (from a controller thread) when the timeline changes. The UI
    // marshals to its own thread and re-queries. Optional; polling also works.
    virtual void SetNoticeCallback(NoticeCallback callback) = 0;
};

// Serialization helpers (stable, versioned; used by the UI and by contract tests).
nlohmann::json ToJson(const TimelineItem& item);
nlohmann::json ToJson(const DetailsPayload& payload);
nlohmann::json ToJson(const AgentStatusView& view);
nlohmann::json ToJson(const ProjectState& state);
nlohmann::json ToJson(const CardAction& action);

// Command name registry (machine-readable list, checked against contracts/registry/commands.json).
std::vector<std::string> CommandNames();
std::vector<std::string> QueryNames();

}  // namespace mayasaba::control
