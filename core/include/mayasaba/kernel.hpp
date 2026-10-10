// Local Execution Kernel (layer 9): the exclusive launch path for Mayasaba-issued processes.
//
// Controlled Win32 process creation: CREATE_SUSPENDED, restricted handle inheritance via
// PROC_THREAD_ATTRIBUTE_HANDLE_LIST, assignment to a controller-owned Job Object before the
// process is resumed, redirected streams with bounded capture, supervised cancellation with
// a deadline and an escalation path, and separately recorded observations.
//
// Job Objects contain process lifecycles and resource limits; they do not restrict files or
// network. Admitted execution modes still require separately proven containment (Policy).
#pragma once

#include <chrono>
#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <vector>

#include "mayasaba/status.hpp"

namespace mayasaba::execution {

struct LaunchSpec {
    std::string executable;                        // absolute path or PATH-resolved name
    std::vector<std::string> arguments;
    std::string working_directory;                 // absolute; must exist
    std::map<std::string, std::string> environment_overrides;
    std::vector<std::string> environment_removals;  // names removed from the inherited block
    std::size_t max_capture_bytes = 4u << 20;      // bounded per-stream capture
    std::size_t active_process_limit = 64;         // Job Object active process cap
    // When set, the process runs under a Windows restricted low-integrity token. The kernel
    // labels the controller-owned workspace tree low integrity before launch, while ordinary
    // user files remain medium integrity; Windows therefore rejects writes outside that tree.
    // This is an OS-enforced material-write boundary, not a prompt or a working-directory
    // convention. It does not, by itself, authorize or prove scoped read access.
    bool require_restricted_workspace = false;
    std::string purpose;                           // recorded (e.g. "agent-session", "test")
};

enum class ProcessOutcome {
    Running,
    Exited,
    TerminatedByRequest,
    TimedOut,
    FailedToLaunch,
    Unknown,
};

const char* ProcessOutcomeName(ProcessOutcome outcome);

struct ProcessObservation {
    ProcessOutcome outcome = ProcessOutcome::Unknown;
    std::uint32_t exit_code = 0;
    bool exit_code_observed = false;
    std::string started_at;
    std::string ended_at;
    std::int64_t duration_ms = 0;
    bool cancellation_requested = false;
    bool termination_observed = false;   // process tree actually gone
    bool escalation_used = false;
    bool restricted_workspace_enforced = false;
    std::string detail;
};

class Process {
public:
    ~Process();
    Process(const Process&) = delete;
    Process& operator=(const Process&) = delete;

    // Launches suspended, assigns to a fresh Job Object, then resumes. Fails closed: any
    // step that cannot be completed rejects the launch and terminates the child.
    static Expected<std::unique_ptr<Process>> Launch(const LaunchSpec& spec);

    Status WriteStdin(const std::string& data);
    void CloseStdin();

    std::string ReadStdout();
    std::string ReadStderr();

    // Waits up to `timeout`; returns Running when the process is still alive.
    ProcessObservation WaitFor(std::chrono::milliseconds timeout);

    // Requests cancellation: terminate the whole job, wait up to `deadline`, escalate when
    // the deadline passes. Requesting cancellation is never reported as proof of stop; the
    // returned observation says what was actually observed.
    ProcessObservation Cancel(std::chrono::milliseconds deadline);

    bool Running();
    std::uint32_t pid() const { return pid_; }
    const ProcessObservation& observation() const { return observation_; }
    bool job_active_processes(std::uint32_t* count);

private:
    Process() = default;
    void StartReaders();
    void StopReaders();
    ProcessObservation FinishObservation(ProcessOutcome outcome, const std::string& detail);

    void* process_ = nullptr;   // HANDLE
    void* job_ = nullptr;       // HANDLE
    void* stdin_write_ = nullptr;
    void* stdout_read_ = nullptr;
    void* stderr_read_ = nullptr;
    std::uint32_t pid_ = 0;
    std::string started_at_;
    std::int64_t start_ms_ = 0;
    ProcessObservation observation_;
    std::size_t max_capture_ = 0;

    struct Stream;
    std::unique_ptr<Stream> stdout_stream_;
    std::unique_ptr<Stream> stderr_stream_;
};

// Helper used by tests and diagnostics: waits for a process id to disappear.
bool WaitForProcessExit(std::uint32_t pid, std::chrono::milliseconds timeout);

}  // namespace mayasaba::execution
