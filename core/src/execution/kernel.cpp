#include "mayasaba/kernel.hpp"

#include <windows.h>
#include <aclapi.h>
#include <sddl.h>

#include <atomic>
#include <mutex>
#include <thread>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::execution {
namespace {

// Windows command-line quoting for a single argument (CRT parsing rules).
std::string QuoteArgument(const std::string& argument) {
    if (argument.empty()) return "\"\"";
    bool needs_quotes = argument.find_first_of(" \t\n\v\"") != std::string::npos;
    std::string out;
    if (!needs_quotes) return argument;
    out.push_back('"');
    std::size_t backslashes = 0;
    for (char c : argument) {
        if (c == '\\') {
            ++backslashes;
            continue;
        }
        if (c == '"') {
            out.append(backslashes * 2 + 1, '\\');
            out.push_back('"');
        } else {
            out.append(backslashes, '\\');
            out.push_back(c);
        }
        backslashes = 0;
    }
    out.append(backslashes * 2, '\\');
    out.push_back('"');
    return out;
}

std::string BuildCommandLine(const std::string& executable,
                             const std::vector<std::string>& arguments) {
    std::string line = QuoteArgument(executable);
    for (const auto& argument : arguments) {
        line.push_back(' ');
        line += QuoteArgument(argument);
    }
    return line;
}

// Builds a UNICODE environment block: inherited environment, minus removals, plus overrides.
std::vector<wchar_t> BuildEnvironmentBlock(const LaunchSpec& spec) {
    std::map<std::wstring, std::wstring> environment;
    if (wchar_t* block = GetEnvironmentStringsW()) {
        for (wchar_t* entry = block; *entry;) {
            std::wstring line(entry);
            entry += line.size() + 1;
            std::size_t equals = line.find(L'=');
            if (equals == std::wstring::npos || equals == 0) continue;
            environment[line.substr(0, equals)] = line.substr(equals + 1);
        }
        FreeEnvironmentStringsW(block);
    }
    for (const auto& name : spec.environment_removals) environment.erase(Utf8ToWide(name));
    for (const auto& [name, value] : spec.environment_overrides) {
        environment[Utf8ToWide(name)] = Utf8ToWide(value);
    }
    std::vector<wchar_t> out;
    for (const auto& [name, value] : environment) {
        std::wstring line = name + L"=" + value;
        out.insert(out.end(), line.begin(), line.end());
        out.push_back(L'\0');
    }
    out.push_back(L'\0');
    return out;
}

struct HandleGuard {
    HANDLE handle = nullptr;
    HandleGuard() = default;
    explicit HandleGuard(HANDLE h) : handle(h) {}
    ~HandleGuard() { if (handle) CloseHandle(handle); }
    HandleGuard(const HandleGuard&) = delete;
    HandleGuard& operator=(const HandleGuard&) = delete;
    HandleGuard(HandleGuard&& other) noexcept : handle(other.release()) {}
    HandleGuard& operator=(HandleGuard&& other) noexcept {
        if (this != &other) {
            if (handle) CloseHandle(handle);
            handle = other.release();
        }
        return *this;
    }
    HANDLE release() {
        HANDLE h = handle;
        handle = nullptr;
        return h;
    }
    operator HANDLE() const { return handle; }
};

// A low-integrity process cannot write to ordinary medium-integrity files. The controller
// relabels its staging tree before launch, while every normal user file retains its existing
// integrity label. This enforces a material-write boundary in Windows rather than relying on a
// working-directory convention. Reparse points are never followed.
Status ApplyLowIntegrityLabel(const std::wstring& path, bool directory) {
    const wchar_t* sddl = directory ? L"S:(ML;OICI;NW;;;LW)" : L"S:(ML;;NW;;;LW)";
    PSECURITY_DESCRIPTOR descriptor = nullptr;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, SDDL_REVISION_1, &descriptor,
                                                               nullptr)) {
        return Status::Error(ErrorCode::Internal,
                             "cannot create low-integrity label: " +
                                 std::to_string(GetLastError()));
    }
    BOOL present = FALSE;
    BOOL defaulted = FALSE;
    PACL sacl = nullptr;
    const bool read_ok = GetSecurityDescriptorSacl(descriptor, &present, &sacl, &defaulted);
    if (!read_ok || !present || sacl == nullptr) {
        LocalFree(descriptor);
        return Status::Error(ErrorCode::Internal, "cannot read low-integrity label");
    }
    const DWORD status = SetNamedSecurityInfoW(const_cast<LPWSTR>(path.c_str()), SE_FILE_OBJECT,
                                                LABEL_SECURITY_INFORMATION, nullptr, nullptr,
                                                nullptr, sacl);
    LocalFree(descriptor);
    if (status != ERROR_SUCCESS) {
        return Status::Error(ErrorCode::IoError,
                             "cannot apply workspace integrity label: " +
                                 std::to_string(status));
    }
    return Status::Ok();
}

Status ApplyLowIntegrityLabelTree(const std::wstring& root) {
    auto root_status = ApplyLowIntegrityLabel(root, true);
    if (!root_status.ok()) return root_status;

    WIN32_FIND_DATAW found{};
    const std::wstring pattern = root + L"\\*";
    HANDLE search = FindFirstFileW(pattern.c_str(), &found);
    if (search == INVALID_HANDLE_VALUE) {
        return Status::Error(ErrorCode::IoError,
                             "cannot enumerate workspace for containment: " +
                                 std::to_string(GetLastError()));
    }
    HandleGuard search_guard(search);
    do {
        const std::wstring name(found.cFileName);
        if (name == L"." || name == L"..") continue;
        if ((found.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) continue;
        const std::wstring child = root + L"\\" + name;
        const bool child_is_directory =
            (found.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        auto child_status = ApplyLowIntegrityLabel(child, child_is_directory);
        if (!child_status.ok()) return child_status;
        if (child_is_directory) {
            child_status = ApplyLowIntegrityLabelTree(child);
            if (!child_status.ok()) return child_status;
        }
    } while (FindNextFileW(search, &found));
    const DWORD error = GetLastError();
    if (error != ERROR_NO_MORE_FILES) {
        return Status::Error(ErrorCode::IoError,
                             "workspace enumeration failed: " + std::to_string(error));
    }
    return Status::Ok();
}

Status CreateLowIntegrityToken(HANDLE* output) {
    if (output == nullptr) {
        return Status::Error(ErrorCode::InvalidArgument, "restricted-token output is null");
    }
    *output = nullptr;
    HANDLE current_token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE |
                                                   TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                          &current_token)) {
        return Status::Error(ErrorCode::Internal,
                             "OpenProcessToken failed: " + std::to_string(GetLastError()));
    }
    HandleGuard current_token_guard(current_token);

    HANDLE restricted_token = nullptr;
    if (!CreateRestrictedToken(current_token, DISABLE_MAX_PRIVILEGE, 0, nullptr, 0, nullptr, 0,
                               nullptr, &restricted_token)) {
        return Status::Error(ErrorCode::Unsupported,
                             "cannot create restricted execution token: " +
                                 std::to_string(GetLastError()));
    }
    DWORD sid_bytes = SECURITY_MAX_SID_SIZE;
    std::vector<BYTE> low_sid(sid_bytes);
    if (!CreateWellKnownSid(WinLowLabelSid, nullptr, low_sid.data(), &sid_bytes)) {
        CloseHandle(restricted_token);
        return Status::Error(ErrorCode::Internal,
                             "CreateWellKnownSid(LowLabel) failed: " +
                                 std::to_string(GetLastError()));
    }
    const DWORD label_bytes = static_cast<DWORD>(sizeof(TOKEN_MANDATORY_LABEL) + sid_bytes);
    std::vector<BYTE> label_storage(label_bytes);
    auto* label = reinterpret_cast<TOKEN_MANDATORY_LABEL*>(label_storage.data());
    label->Label.Sid = low_sid.data();
    label->Label.Attributes = SE_GROUP_INTEGRITY;
    if (!SetTokenInformation(restricted_token, TokenIntegrityLevel, label, label_bytes)) {
        const DWORD error = GetLastError();
        CloseHandle(restricted_token);
        return Status::Error(ErrorCode::Unsupported,
                             "cannot set low-integrity execution token: " +
                                 std::to_string(error));
    }
    DWORD verified_bytes = 0;
    GetTokenInformation(restricted_token, TokenIntegrityLevel, nullptr, 0, &verified_bytes);
    std::vector<BYTE> verified_storage(verified_bytes);
    if (verified_bytes == 0 ||
        !GetTokenInformation(restricted_token, TokenIntegrityLevel, verified_storage.data(),
                             verified_bytes, &verified_bytes)) {
        const DWORD error = GetLastError();
        CloseHandle(restricted_token);
        return Status::Error(ErrorCode::Internal,
                             "cannot verify low-integrity execution token: " +
                                 std::to_string(error));
    }
    const auto* verified =
        reinterpret_cast<const TOKEN_MANDATORY_LABEL*>(verified_storage.data());
    if (!EqualSid(verified->Label.Sid, low_sid.data())) {
        CloseHandle(restricted_token);
        return Status::Error(ErrorCode::Internal,
                             "execution token did not retain the low-integrity label");
    }
    *output = restricted_token;
    return Status::Ok();
}

}  // namespace

const char* ProcessOutcomeName(ProcessOutcome outcome) {
    switch (outcome) {
        case ProcessOutcome::Running: return "RUNNING";
        case ProcessOutcome::Exited: return "EXITED";
        case ProcessOutcome::TerminatedByRequest: return "TERMINATED_BY_REQUEST";
        case ProcessOutcome::TimedOut: return "TIMED_OUT";
        case ProcessOutcome::FailedToLaunch: return "FAILED_TO_LAUNCH";
        case ProcessOutcome::Unknown: return "UNKNOWN";
    }
    return "UNKNOWN";
}

struct Process::Stream {
    HANDLE pipe = nullptr;         // read end (for stdout/stderr)
    HANDLE write_end = nullptr;    // child's end, closed in the parent after launch
    std::thread reader;
    std::mutex mutex;
    std::string data;
    std::size_t limit = 0;
    std::atomic<bool> done{false};
};

Process::~Process() {
    CloseStdin();
    if (Running()) {
        if (job_) TerminateJobObject(job_, 1);
        if (process_) WaitForSingleObject(process_, 3000);
        observation_.termination_observed = true;
        observation_.outcome = ProcessOutcome::TerminatedByRequest;
        observation_.detail = "terminated during cleanup";
    }
    StopReaders();
    if (stdin_write_) CloseHandle(stdin_write_);
    if (stdout_read_) CloseHandle(stdout_read_);
    if (stderr_read_) CloseHandle(stderr_read_);
    if (process_) CloseHandle(process_);
    if (job_) CloseHandle(job_);
}

void Process::StartReaders() {
    auto start = [this](Stream* stream) {
        stream->reader = std::thread([stream] {
            std::vector<char> buffer(1 << 16);
            for (;;) {
                DWORD read = 0;
                BOOL ok = ReadFile(stream->pipe, buffer.data(),
                                   static_cast<DWORD>(buffer.size()), &read, nullptr);
                if (!ok || read == 0) break;
                std::lock_guard<std::mutex> lock(stream->mutex);
                if (stream->data.size() < stream->limit) {
                    std::size_t room = stream->limit - stream->data.size();
                    stream->data.append(buffer.data(), std::min<std::size_t>(room, read));
                }
                // Beyond the limit the stream is drained and discarded: the child is never
                // blocked by a full pipe, and capture stays bounded.
            }
            stream->done = true;
        });
    };
    if (stdout_stream_) start(stdout_stream_.get());
    if (stderr_stream_) start(stderr_stream_.get());
}

void Process::StopReaders() {
    auto stop = [](Stream* stream) {
        if (!stream) return;
        if (stream->reader.joinable()) {
            // Break a blocked ReadFile, then join.
            CancelIoEx(stream->pipe, nullptr);
            stream->reader.join();
        }
    };
    stop(stdout_stream_.get());
    stop(stderr_stream_.get());
}

Expected<std::unique_ptr<Process>> Process::Launch(const LaunchSpec& spec) {
    std::unique_ptr<Process> process(new Process());
    process->max_capture_ = spec.max_capture_bytes;

    if (spec.executable.empty()) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::InvalidArgument, "empty executable");
    }
    auto executable_path = fs::CanonicalizePath(spec.executable);
    if (!executable_path.ok()) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::NotFound,
                                              "executable not found: " + spec.executable);
    }
    auto identity = fs::IdentifyPath(executable_path.value());
    if (!identity.ok() || !identity.value().exists) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::NotFound,
                                              "executable not found: " + spec.executable);
    }
    std::wstring working_directory = Utf8ToWide(spec.working_directory);
    if (working_directory.empty() || !fs::PathExists(spec.working_directory)) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::NotFound,
                                              "working directory not found: " +
                                                  spec.working_directory);
    }

    HandleGuard restricted_token;
    if (spec.require_restricted_workspace) {
        auto acl_status = ApplyLowIntegrityLabelTree(working_directory);
        if (!acl_status.ok()) {
            return Fail<std::unique_ptr<Process>>(ErrorCode::Blocked,
                                                  "workspace containment setup failed: " +
                                                      acl_status.message());
        }
        HANDLE token = nullptr;
        auto token_status = CreateLowIntegrityToken(&token);
        if (!token_status.ok()) {
            return Fail<std::unique_ptr<Process>>(token_status.code(), token_status.message());
        }
        restricted_token = HandleGuard(token);
    }

    // Pipes: parent reads stdout/stderr, writes stdin. Only these handles are inherited.
    HANDLE stdin_read = nullptr, stdin_write = nullptr;
    HANDLE stdout_read = nullptr, stdout_write = nullptr;
    HANDLE stderr_read = nullptr, stderr_write = nullptr;
    SECURITY_ATTRIBUTES attributes{sizeof(SECURITY_ATTRIBUTES), nullptr, TRUE};
    if (!CreatePipe(&stdin_read, &stdin_write, &attributes, 0) ||
        !CreatePipe(&stdout_read, &stdout_write, &attributes, 0) ||
        !CreatePipe(&stderr_read, &stderr_write, &attributes, 0)) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::IoError, "pipe creation failed");
    }
    HandleGuard guard_stdin_read(stdin_read), guard_stdin_write(stdin_write),
        guard_stdout_read(stdout_read), guard_stdout_write(stdout_write),
        guard_stderr_read(stderr_read), guard_stderr_write(stderr_write);
    SetHandleInformation(stdin_write, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(stdout_read, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(stderr_read, HANDLE_FLAG_INHERIT, 0);

    // Job Object with kill-on-close and an active-process limit.
    HANDLE job = CreateJobObjectW(nullptr, nullptr);
    if (!job) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::Internal, "CreateJobObject failed");
    }
    HandleGuard guard_job(job);
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (spec.active_process_limit > 0) {
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit =
            static_cast<DWORD>(spec.active_process_limit);
    }
    if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits,
                                 sizeof(limits))) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::Internal,
                                              "SetInformationJobObject failed");
    }

    // Restricted handle inheritance: only the three child-side pipe handles.
    std::vector<HANDLE> inherited{stdin_read, stdout_write, stderr_write};
    SIZE_T attribute_bytes = 0;
    InitializeProcThreadAttributeList(nullptr, 1, 0, &attribute_bytes);
    std::vector<char> attribute_buffer(attribute_bytes);
    LPPROC_THREAD_ATTRIBUTE_LIST attribute_list =
        reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(attribute_buffer.data());
    if (!InitializeProcThreadAttributeList(attribute_list, 1, 0, &attribute_bytes) ||
        !UpdateProcThreadAttribute(attribute_list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                                   inherited.data(), inherited.size() * sizeof(HANDLE), nullptr,
                                   nullptr)) {
        return Fail<std::unique_ptr<Process>>(ErrorCode::Internal,
                                              "handle-list attribute setup failed");
    }

    STARTUPINFOEXW startup{};
    startup.StartupInfo.cb = sizeof(startup);
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    startup.StartupInfo.wShowWindow = SW_HIDE;
    startup.StartupInfo.hStdInput = stdin_read;
    startup.StartupInfo.hStdOutput = stdout_write;
    startup.StartupInfo.hStdError = stderr_write;
    startup.lpAttributeList = attribute_list;

    auto environment = BuildEnvironmentBlock(spec);
    std::string command_line = BuildCommandLine(executable_path.value(), spec.arguments);
    std::wstring command_line_wide = Utf8ToWide(command_line);
    std::wstring executable_wide = Utf8ToWide(executable_path.value());

    PROCESS_INFORMATION info{};
    DWORD flags = CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT |
                  EXTENDED_STARTUPINFO_PRESENT;
    BOOL created = spec.require_restricted_workspace
                       ? CreateProcessAsUserW(restricted_token, executable_wide.c_str(),
                                              command_line_wide.data(), nullptr, nullptr, TRUE,
                                              flags, environment.data(), working_directory.c_str(),
                                              &startup.StartupInfo, &info)
                       : CreateProcessW(executable_wide.c_str(), command_line_wide.data(), nullptr,
                                        nullptr, TRUE, flags, environment.data(),
                                        working_directory.c_str(), &startup.StartupInfo, &info);
    DeleteProcThreadAttributeList(attribute_list);
    if (!created) {
        return Fail<std::unique_ptr<Process>>(
            ErrorCode::IoError,
            "CreateProcessW failed with error " + std::to_string(GetLastError()));
    }
    HandleGuard guard_thread(info.hThread);

    // Assign to the job before resuming. Assignment failure rejects the launch.
    if (!AssignProcessToJobObject(job, info.hProcess)) {
        TerminateProcess(info.hProcess, 1);
        CloseHandle(info.hProcess);
        return Fail<std::unique_ptr<Process>>(ErrorCode::Internal,
                                              "job assignment failed; launch rejected");
    }
    if (ResumeThread(info.hThread) == static_cast<DWORD>(-1)) {
        TerminateJobObject(job, 1);
        CloseHandle(info.hProcess);
        return Fail<std::unique_ptr<Process>>(ErrorCode::Internal, "ResumeThread failed");
    }

    process->process_ = info.hProcess;
    process->job_ = guard_job.release();
    process->pid_ = info.dwProcessId;
    process->stdin_write_ = guard_stdin_write.release();
    process->stdout_read_ = guard_stdout_read.release();
    process->stderr_read_ = guard_stderr_read.release();
    process->started_at_ = NowUtcIso8601();
    process->start_ms_ = MonotonicMillis();
    process->observation_.outcome = ProcessOutcome::Running;
    process->observation_.started_at = process->started_at_;
    process->observation_.restricted_workspace_enforced = spec.require_restricted_workspace;

    process->stdout_stream_ = std::make_unique<Stream>();
    process->stdout_stream_->pipe = process->stdout_read_;
    process->stdout_stream_->limit = spec.max_capture_bytes;
    process->stderr_stream_ = std::make_unique<Stream>();
    process->stderr_stream_->pipe = process->stderr_read_;
    process->stderr_stream_->limit = spec.max_capture_bytes;
    process->StartReaders();

    // Close the child-side ends in the parent so EOF is observable.
    CloseHandle(stdin_read);
    guard_stdin_read.release();
    CloseHandle(stdout_write);
    guard_stdout_write.release();
    CloseHandle(stderr_write);
    guard_stderr_write.release();
    return process;
}

Status Process::WriteStdin(const std::string& data) {
    if (!stdin_write_) return Status::Error(ErrorCode::InvalidArgument, "stdin is closed");
    std::size_t offset = 0;
    while (offset < data.size()) {
        DWORD written = 0;
        DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(data.size() - offset, 1u << 16));
        if (!WriteFile(stdin_write_, data.data() + offset, chunk, &written, nullptr) ||
            written == 0) {
            return Status::Error(ErrorCode::IoError, "stdin write failed");
        }
        offset += written;
    }
    return Status::Ok();
}

void Process::CloseStdin() {
    if (stdin_write_) {
        CloseHandle(stdin_write_);
        stdin_write_ = nullptr;
    }
}

std::string Process::ReadStdout() {
    if (!stdout_stream_) return {};
    std::lock_guard<std::mutex> lock(stdout_stream_->mutex);
    return stdout_stream_->data;
}

std::string Process::ReadStderr() {
    if (!stderr_stream_) return {};
    std::lock_guard<std::mutex> lock(stderr_stream_->mutex);
    return stderr_stream_->data;
}

bool Process::Running() {
    if (!process_) return false;
    return WaitForSingleObject(process_, 0) == WAIT_TIMEOUT;
}

ProcessObservation Process::FinishObservation(ProcessOutcome outcome, const std::string& detail) {
    observation_.outcome = outcome;
    observation_.detail = detail;
    observation_.ended_at = NowUtcIso8601();
    observation_.duration_ms = MonotonicMillis() - start_ms_;
    return observation_;
}

ProcessObservation Process::WaitFor(std::chrono::milliseconds timeout) {
    if (!process_) {
        return FinishObservation(ProcessOutcome::FailedToLaunch, "no process handle");
    }
    DWORD wait = WaitForSingleObject(process_, static_cast<DWORD>(timeout.count()));
    if (wait == WAIT_TIMEOUT) {
        return observation_;  // still Running; not a failure
    }
    if (wait == WAIT_FAILED) {
        return FinishObservation(ProcessOutcome::Unknown, "WaitForSingleObject failed");
    }
    DWORD exit_code = 0;
    bool observed = GetExitCodeProcess(process_, &exit_code) != 0;
    observation_.exit_code = exit_code;
    observation_.exit_code_observed = observed;
    observation_.termination_observed = true;
    // A process that ran to completion and exited is "Exited"; the exit code is data, not a
    // verdict (an exit code alone never proves task success).
    return FinishObservation(ProcessOutcome::Exited, "process exited");
}

ProcessObservation Process::Cancel(std::chrono::milliseconds deadline) {
    if (!process_) {
        return FinishObservation(ProcessOutcome::FailedToLaunch, "no process handle");
    }
    observation_.cancellation_requested = true;
    if (job_) {
        TerminateJobObject(job_, 1);
    } else {
        TerminateProcess(process_, 1);
    }
    DWORD wait = WaitForSingleObject(process_, static_cast<DWORD>(deadline.count()));
    if (wait == WAIT_OBJECT_0) {
        DWORD exit_code = 0;
        observation_.exit_code_observed = GetExitCodeProcess(process_, &exit_code) != 0;
        observation_.exit_code = exit_code;
        observation_.termination_observed = true;
        return FinishObservation(ProcessOutcome::TerminatedByRequest,
                                 "termination observed within deadline");
    }
    // Escalation: the deadline passed. Record that escalation was used and try once more.
    observation_.escalation_used = true;
    if (job_) TerminateJobObject(job_, 2);
    TerminateProcess(process_, 2);
    wait = WaitForSingleObject(process_, 2000);
    observation_.termination_observed = (wait == WAIT_OBJECT_0);
    return FinishObservation(ProcessOutcome::TerminatedByRequest,
                             observation_.termination_observed
                                 ? "termination observed after escalation"
                                 : "termination not observed after escalation");
}

bool Process::job_active_processes(std::uint32_t* count) {
    if (!job_) return false;
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION info{};
    if (!QueryInformationJobObject(job_, JobObjectBasicAccountingInformation, &info,
                                   sizeof(info), nullptr)) {
        return false;
    }
    if (count) *count = info.ActiveProcesses;
    return true;
}

bool WaitForProcessExit(std::uint32_t pid, std::chrono::milliseconds timeout) {
    HANDLE handle = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!handle) return true;  // already gone
    DWORD wait = WaitForSingleObject(handle, static_cast<DWORD>(timeout.count()));
    CloseHandle(handle);
    return wait == WAIT_OBJECT_0;
}

}  // namespace mayasaba::execution
