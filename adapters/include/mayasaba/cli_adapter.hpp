// CLI adapter implementation: one instance per agent, the only place that CLI's native
// protocol exists. Launch profiles are explicit, probe-gated and never carry model/provider
// selection; forbidden flags (approval bypass, model/provider overrides, updates, credential
// export) are rejected at profile construction and asserted before every launch.
#pragma once

#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "mayasaba/adapter.hpp"
#include "mayasaba/kernel.hpp"

namespace mayasaba::adapters {

// A verified launch profile. `prompt_via_stdin` writes the prompt to the child's stdin
// (used where the CLI supports reading the query from stdin, avoiding command-line length
// and quoting limits). `emits_json_lines` selects native JSON event-stream parsing.
struct CliProfile {
    AgentKind kind = AgentKind::Hermes;
    std::vector<std::string> probe_args;          // bounded, non-mutating help probe
    std::vector<std::string> launch_args;         // headless session vector
    bool prompt_via_stdin = false;
    bool emits_json_lines = true;
    bool verified_on_this_machine = false;        // set only by a real observed probe
    std::string notes;                            // recorded rationale / limitations
};

// The three profiles: Hermes, Kilo Code, and Claude Code. Launch vectors are observed from
// the installed CLIs' own help interfaces (bounded, non-mutating).
CliProfile DefaultProfile(AgentKind kind);

// Rejects profiles that request forbidden behavior: approval bypass, model/provider
// selection, update/install, credential export, plugin installation, arbitrary serving.
Status ValidateProfile(const CliProfile& profile);

class CliAdapter : public Adapter {
public:
    // `explicit_path` comes from Mayasaba-owned settings ("Locate executable"); it never
    // installs or modifies anything. `profile_override` is for tests and verified overrides.
    CliAdapter(AgentKind kind, std::string explicit_path = {}, CliProfile profile_override = {});
    ~CliAdapter() override;

    AgentKind kind() const override { return kind_; }

    Expected<AdapterStatus> Probe() override;
    Status StartSession(const SessionSpec& spec, const std::string& session_id) override;
    Status SendPrompt(const std::string& session_id, const std::string& prompt) override;
    Status Cancel(const std::string& session_id, std::string* observed_reason) override;
    Status Stop(const std::string& session_id) override;
    std::vector<SessionEvent> DrainEvents(const std::string& session_id) override;
    bool HasSession(const std::string& session_id) const override;

    // Session observation for the gateway service: the process outcome as actually observed.
    struct SessionSnapshot {
        std::string session_id;
        std::string state;  // STARTING | RUNNING | EXITED | CANCELLED | FAILED | UNKNOWN
        execution::ProcessObservation observation;
        std::size_t event_count = 0;
    };
    bool Snapshot(const std::string& session_id, SessionSnapshot* snapshot);

    const AdapterStatus& last_status() const { return status_; }
    const CliProfile& profile() const { return profile_; }
    const std::string& executable_path() const { return executable_path_; }

private:
    struct Session;
    Status LaunchSession(const SessionSpec& spec, const std::string& session_id,
                         const std::vector<std::string>& arguments, bool write_prompt_stdin);

    AgentKind kind_;
    std::string explicit_path_;
    CliProfile profile_;
    std::string executable_path_;
    AdapterStatus status_;
    mutable std::mutex mutex_;
    std::map<std::string, std::unique_ptr<Session>> sessions_;
};

}  // namespace mayasaba::adapters
