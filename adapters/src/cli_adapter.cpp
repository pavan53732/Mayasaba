// CLI adapter implementation: one instance per agent; the only place that CLI's native
// protocol exists. Launch profiles are explicit, probe-gated, and never carry model/provider
// selection. Forbidden flags (approval bypass, model/provider overrides, updates, credential
// export, external messaging, serving) are rejected at profile validation and re-checked
// before every launch.
#include "mayasaba/cli_adapter.hpp"

#include <algorithm>
#include <chrono>
#include <thread>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::adapters {
namespace {

// Exact-match flags/verbs that must never appear in a probe or session launch vector.
const char* kForbiddenArguments[] = {
    // Approval bypass / dangerous automation.
    "--yolo", "--auto", "--dangerously-skip-permissions", "--dangerously-bypass-approvals-and-sandbox",
    "--force", "--accept-hooks", "--no-confirm",
    // Model / provider / reasoning selection (user-owned; Mayasaba never overrides).
    "-m", "--model", "--provider", "--reasoning", "--agent", "--thinking", "--effort",
    // CLI self-modification and account/credential operations.
    "upgrade", "update", "uninstall", "install", "plugin", "plugins", "logout", "login",
    "auth", "secrets", "config", "token",
    // External messaging / serving / proxies.
    "serve", "gateway", "send", "whatsapp", "slack", "discord", "webhook", "peer", "proxy",
};

bool ContainsForbidden(const std::string& argument, std::string* matched) {
    for (const char* forbidden : kForbiddenArguments) {
        if (argument == forbidden) {
            if (matched) *matched = forbidden;
            return true;
        }
    }
    return false;
}

std::string TrimCarriageReturn(std::string line) {
    if (!line.empty() && line.back() == '\r') line.pop_back();
    return line;
}

// Extracts a human-readable text field from a native JSON event, if present.
std::string ExtractEventText(const nlohmann::json& value) {
    static const char* kTextKeys[] = {"text", "content", "message", "result", "output",
                                      "delta", "summary"};
    for (const char* key : kTextKeys) {
        if (value.contains(key) && value[key].is_string()) {
            return RedactSecrets(value[key].get<std::string>());
        }
        if (value.contains(key) && value[key].is_object()) {
            const auto& nested = value[key];
            for (const char* nested_key : {"text", "content"}) {
                if (nested.contains(nested_key) && nested[nested_key].is_string()) {
                    return RedactSecrets(nested[nested_key].get<std::string>());
                }
            }
        }
    }
    return {};
}

}  // namespace

CliProfile DefaultProfile(AgentKind kind) {
    CliProfile profile;
    profile.kind = kind;
    switch (kind) {
        case AgentKind::Hermes:
            // Observed from the installed CLI's own help output (bounded, non-mutating):
            //   hermes chat [-q QUERY] [--query-file PATH] [--format stream-json] [-Q]
            //               [--ignore-rules] [--oneshot]
            // --query-file '-' reads the query from stdin and is documented as safe for
            // arbitrary text (nothing is shell-interpreted). --format stream-json is the
            // native JSON event stream; -Q is quiet/programmatic mode; --ignore-rules
            // suppresses rule-file injection from outside the authorized workspace.
            profile.probe_args = {"chat", "--help"};
            profile.launch_args = {"chat", "--query-file", "-", "--format", "stream-json", "-Q",
                                   "--ignore-rules"};
            profile.prompt_via_stdin = true;
            profile.emits_json_lines = true;
            profile.verified_on_this_machine = true;
            profile.notes =
                "Hermes: launch vector verified against the installed CLI's observed help "
                "interface. stdin query + native stream-json + rule-file injection suppressed. "
                "Model/provider/reasoning flags are never passed; the CLI resolves its own "
                "user-managed configuration.";
            break;
        case AgentKind::Kilo:
            // Observed from the installed CLI's own help output (2026-10-10, bounded,
            // non-mutating):
            //   kilo run [message..] with --format json (raw JSON events), -i default false.
            // The Kilo Code CLI is OpenCode-compatible (same command structure). Flags that
            // select model/provider/reasoning (--model, --agent, --variant, --thinking) or
            // share/serve (--share, --attach) are never passed.
            profile.probe_args = {"run", "--help"};
            profile.launch_args = {"run", "--format", "json", "{prompt}"};
            profile.prompt_via_stdin = false;
            profile.emits_json_lines = true;
            profile.verified_on_this_machine = true;
            profile.notes =
                "Kilo Code: launch vector verified against the installed CLI's observed help "
                "interface (OpenCode-compatible). Native json output; non-interactive by "
                "default. Auto-approve/share/serve flags are never passed. Prompt is a "
                "positional argument, subject to the Windows command-line length limit.";
            break;
        case AgentKind::OpenCode:
            // Observed from the installed CLI's own help output (bounded, non-mutating):
            //   opencode run [flags] [<message...>] with --standalone (private server
            //   instead of the shared background service) and --format json (native JSON
            //   output). --auto (auto-approve) is forbidden and never passed.
            profile.probe_args = {"run", "--help"};
            profile.launch_args = {"run", "--standalone", "--format", "json", "{prompt}"};
            profile.prompt_via_stdin = false;
            profile.emits_json_lines = true;
            profile.verified_on_this_machine = true;
            profile.notes =
                "OpenCode: launch vector verified against the installed CLI's observed help "
                "interface. Isolated private server (--standalone) + native json output. "
                "Auto-approve is never passed. Prompt is a positional argument, subject to "
                "the Windows command-line length limit.";
            break;
    }
    return profile;
}

Status ValidateProfile(const CliProfile& profile) {
    auto check = [&](const std::vector<std::string>& args, const char* what) {
        for (const auto& argument : args) {
            std::string matched;
            if (ContainsForbidden(argument, &matched)) {
                return Status::Error(ErrorCode::Denied,
                                     std::string("forbidden flag in ") + what +
                                         " launch vector: " + matched);
            }
        }
        return Status::Ok();
    };
    auto status = check(profile.probe_args, "probe");
    if (!status.ok()) return status;
    return check(profile.launch_args, "session");
}

struct CliAdapter::Session {
    std::string session_id;
    std::unique_ptr<execution::Process> process;
    std::mutex mutex;
    std::vector<SessionEvent> buffered;      // produced, not yet drained
    std::string state = "STARTING";
    execution::ProcessObservation observation;
    std::size_t consumed_bytes = 0;          // how much of the accumulated stdout was parsed
    std::string partial_line;                // trailing bytes of an incomplete line
    std::size_t parse_failures = 0;
    std::size_t parseable_events = 0;
};

CliAdapter::CliAdapter(AgentKind kind, std::string explicit_path, CliProfile profile_override)
    : kind_(kind), explicit_path_(std::move(explicit_path)) {
    profile_ =
        profile_override.launch_args.empty() ? DefaultProfile(kind) : std::move(profile_override);
    status_.agent = kind;
    status_.readiness = Readiness::Checking;
}

CliAdapter::~CliAdapter() {
    std::lock_guard<std::mutex> lock(mutex_);
    sessions_.clear();  // Process destructors terminate any surviving children
}

Expected<AdapterStatus> CliAdapter::Probe() {
    auto validation = ValidateProfile(profile_);
    if (!validation.ok()) return Expected<AdapterStatus>(validation);

    AdapterStatus status;
    status.agent = kind_;
    status.probed_at = NowUtcIso8601();

    auto discovery = DiscoverExecutable(kind_, explicit_path_);
    if (discovery.path.empty()) {
        status.readiness = Readiness::Missing;
        status.reason = "no executable found; searched: ";
        for (std::size_t i = 0; i < discovery.searched.size() && i < 6; ++i) {
            if (i) status.reason += ", ";
            status.reason += discovery.searched[i];
        }
        std::lock_guard<std::mutex> lock(mutex_);
        status_ = status;
        return status;
    }
    status.executable_path = discovery.path;
    auto identity = fs::IdentifyPath(discovery.path);
    if (identity.ok() && identity.value().exists && !identity.value().is_directory) {
        auto digest = Sha256::HexOfFile(discovery.path);
        if (digest.ok()) status.executable_sha256 = digest.value();
    }

    // Bounded, non-mutating behavioral probe: the CLI's own help. Consumes no model usage,
    // changes nothing, observes that the executable launches and answers.
    execution::LaunchSpec spec;
    spec.executable = discovery.path;
    spec.arguments = profile_.probe_args;
    spec.working_directory = fs::ParentPath(discovery.path);
    if (spec.working_directory.empty()) spec.working_directory = "C:\\";
    spec.max_capture_bytes = 512 * 1024;
    spec.purpose = "agent-probe:" + std::string(AgentId(kind_));
    auto process = execution::Process::Launch(spec);
    if (!process.ok()) {
        status.readiness = Readiness::ProbeFailed;
        status.reason = "launch failed: " + process.message();
        std::lock_guard<std::mutex> lock(mutex_);
        status_ = status;
        return status;
    }
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(20000));
    if (observation.outcome == execution::ProcessOutcome::Running) {
        process.value()->Cancel(std::chrono::milliseconds(3000));
        status.readiness = Readiness::ProbeFailed;
        status.reason = "probe did not complete within 20s";
    } else if (observation.outcome != execution::ProcessOutcome::Exited) {
        status.readiness = Readiness::ProbeFailed;
        status.reason = std::string("probe ended as ") +
                        execution::ProcessOutcomeName(observation.outcome) + ": " +
                        observation.detail;
    } else if (observation.exit_code != 0) {
        status.readiness = Readiness::ProbeFailed;
        status.reason = "probe exited with code " + std::to_string(observation.exit_code);
    } else {
        std::string help_output = process.value()->ReadStdout() + process.value()->ReadStderr();
        if (help_output.size() < 16) {
            status.readiness = Readiness::ProbeFailed;
            status.reason = "probe produced no observable output";
        } else {
            status.readiness = Readiness::Ready;
            status.reason.clear();
            status.observed_capabilities = {
                {"help_output_bytes", help_output.size()},
                {"probe_args", profile_.probe_args},
                {"json_event_stream", profile_.emits_json_lines},
                {"prompt_via_stdin", profile_.prompt_via_stdin},
                {"profile_verified", profile_.verified_on_this_machine},
            };
        }
    }
    std::lock_guard<std::mutex> lock(mutex_);
    executable_path_ = discovery.path;
    status_ = status;
    return status;
}

Status CliAdapter::LaunchSession(const SessionSpec& spec, const std::string& session_id,
                                 const std::vector<std::string>& arguments,
                                 bool write_prompt_stdin) {
    execution::LaunchSpec launch;
    launch.executable = executable_path_;
    launch.arguments = arguments;
    launch.working_directory = spec.execution_working_directory;
    launch.max_capture_bytes = 8u << 20;
    launch.purpose = "agent-session:" + std::string(AgentId(kind_));
    auto process = execution::Process::Launch(launch);
    if (!process.ok()) return process.status();

    if (write_prompt_stdin) {
        auto status = process.value()->WriteStdin(spec.prompt);
        if (status.ok() && (spec.prompt.empty() || spec.prompt.back() != '\n')) {
            status = process.value()->WriteStdin("\n");
        }
        process.value()->CloseStdin();
        if (!status.ok()) {
            process.value()->Cancel(std::chrono::milliseconds(2000));
            return status;
        }
    }

    auto session = std::make_unique<Session>();
    session->session_id = session_id;
    session->process = std::move(process.value());
    session->state = "RUNNING";
    session->observation.outcome = execution::ProcessOutcome::Running;
    session->observation.started_at = NowUtcIso8601();

    std::lock_guard<std::mutex> lock(mutex_);
    sessions_[session_id] = std::move(session);
    return Status::Ok();
}

Status CliAdapter::StartSession(const SessionSpec& spec, const std::string& session_id) {
    if (HasSession(session_id)) {
        return Status::Error(ErrorCode::AlreadyExists, "session already exists: " + session_id);
    }
    if (status_.readiness != Readiness::Ready) {
        return Status::Error(ErrorCode::Unavailable,
                             std::string("agent ") + AgentName(kind_) +
                                 " is not READY for sessions: " +
                                 (status_.reason.empty() ? "no probe has succeeded" : status_.reason));
    }
    if (!profile_.verified_on_this_machine) {
        return Status::Error(ErrorCode::Unsupported,
                             std::string("launch profile for ") + AgentName(kind_) +
                                 " is not verified on this machine: " + profile_.notes);
    }
    if (!fs::PathExists(spec.execution_working_directory)) {
        return Status::Error(ErrorCode::NotFound, "session working directory does not exist");
    }
    if (profile_.launch_args.empty()) {
        return Status::Error(ErrorCode::Unsupported, "no launch profile");
    }
    auto validation = ValidateProfile(profile_);
    if (!validation.ok()) return validation;

    std::vector<std::string> arguments;
    bool prompt_placed = false;
    for (const auto& argument : profile_.launch_args) {
        if (argument == "{prompt}") {
            arguments.push_back(spec.prompt);
            prompt_placed = true;
        } else {
            arguments.push_back(argument);
        }
    }
    if (profile_.prompt_via_stdin) {
        if (prompt_placed) {
            return Status::Error(ErrorCode::Internal, "profile mixes stdin and argv prompts");
        }
    } else if (!prompt_placed) {
        return Status::Error(ErrorCode::Internal, "profile has no prompt placeholder");
    }
    return LaunchSession(spec, session_id, arguments, profile_.prompt_via_stdin);
}

Status CliAdapter::SendPrompt(const std::string& session_id, const std::string& prompt) {
    (void)prompt;
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = sessions_.find(session_id);
    if (it == sessions_.end()) return Status::Error(ErrorCode::NotFound, "no such session");
    if (it->second->process->Running()) {
        return Status::Error(ErrorCode::Unsupported,
                             "this CLI profile is single-shot; a new session is required");
    }
    return Status::Error(ErrorCode::Conflict, "session has already ended");
}

Status CliAdapter::Cancel(const std::string& session_id, std::string* observed_reason) {
    Session* session = nullptr;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = sessions_.find(session_id);
        if (it == sessions_.end()) return Status::Error(ErrorCode::NotFound, "no such session");
        session = it->second.get();
        session->state = "CANCELLING";
    }
    auto observation = session->process->Cancel(std::chrono::milliseconds(5000));
    {
        std::lock_guard<std::mutex> lock(mutex_);
        session->observation = observation;
        session->state = observation.termination_observed ? "CANCELLED" : "UNKNOWN";
    }
    if (observed_reason) *observed_reason = observation.detail;
    return Status::Ok();
}

Status CliAdapter::Stop(const std::string& session_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = sessions_.find(session_id);
    if (it == sessions_.end()) return Status::Error(ErrorCode::NotFound, "no such session");
    it->second->state = "STOPPED";
    sessions_.erase(it);  // Process destructor terminates the tree if still alive
    return Status::Ok();
}

std::vector<SessionEvent> CliAdapter::DrainEvents(const std::string& session_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = sessions_.find(session_id);
    if (it == sessions_.end()) return {};
    Session& session = *it->second;

    // The kernel captures accumulate; parse only newly observed bytes. An incomplete trailing
    // line is kept as a partial buffer until the rest arrives (fragmented streams are normal:
    // pipes split arbitrarily and the last line may not be newline-terminated yet).
    std::string raw = session.process->ReadStdout();
    if (raw.size() > session.consumed_bytes) {
        std::string chunk = session.partial_line + raw.substr(session.consumed_bytes);
        session.consumed_bytes = raw.size();
        session.partial_line.clear();
        std::size_t start = 0;
        while (true) {
            std::size_t end = chunk.find('\n', start);
            if (end == std::string::npos) {
                session.partial_line = chunk.substr(start);
                break;
            }
            std::string line = TrimCarriageReturn(chunk.substr(start, end - start));
            start = end + 1;
            if (line.empty()) continue;
            if (profile_.emits_json_lines) {
                auto parsed = ParseJsonBounded(line, {1u << 20, 32, 4096});
                if (!parsed.ok()) {
                    ++session.parse_failures;  // counted, never silently treated as an event
                    continue;
                }
                SessionEvent event;
                const auto& value = parsed.value();
                event.kind = value.value("type", std::string("message"));
                event.text = ExtractEventText(value);
                event.payload = value;
                ++session.parseable_events;
                session.buffered.push_back(std::move(event));
            } else {
                SessionEvent event;
                event.kind = "message";
                event.text = RedactSecrets(line);
                ++session.parseable_events;
                session.buffered.push_back(std::move(event));
            }
        }
    }

    // Observe completion. A clean exit with no parseable events is a failure, never a success:
    // exit codes are data, not verdicts.
    if (session.process->Running()) {
        session.state = "RUNNING";
    } else {
        auto observation = session.process->WaitFor(std::chrono::milliseconds(0));
        session.observation = observation;
        if (session.state == "CANCELLING") {
            session.state = observation.termination_observed ? "CANCELLED" : "UNKNOWN";
        } else if (session.parseable_events == 0) {
            session.state = "FAILED";
            SessionEvent event;
            event.kind = "error";
            event.text = "process ended without any parseable events (exit code " +
                         std::to_string(observation.exit_code) + "; parse failures " +
                         std::to_string(session.parse_failures) + ")";
            session.buffered.push_back(std::move(event));
        } else {
            session.state = "EXITED";
        }
    }

    std::vector<SessionEvent> out = std::move(session.buffered);
    session.buffered.clear();
    return out;
}

bool CliAdapter::HasSession(const std::string& session_id) const {
    std::lock_guard<std::mutex> lock(mutex_);
    return sessions_.find(session_id) != sessions_.end();
}

bool CliAdapter::Snapshot(const std::string& session_id, SessionSnapshot* snapshot) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = sessions_.find(session_id);
    if (it == sessions_.end()) return false;
    if (snapshot) {
        snapshot->session_id = session_id;
        snapshot->state = it->second->state;
        snapshot->observation = it->second->observation;
        snapshot->event_count = it->second->parseable_events;
    }
    return true;
}

}  // namespace mayasaba::adapters
