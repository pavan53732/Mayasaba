// Agent Gateway implementation (layer 4 service): session identity, health, and the only
// path for adapter-translated native traffic. The gateway never schedules work, never mutates
// task records, and never reduces a three-agent operation to two agents.
#include "mayasaba/gateway.hpp"

#include <algorithm>

#include "mayasaba/base.hpp"

namespace mayasaba::gateway {

using adapters::AdapterStatus;
using adapters::AgentId;
using adapters::AgentKind;
using adapters::AllAgents;
using adapters::CliAdapter;
using adapters::Readiness;
using adapters::ReadinessName;
using adapters::SessionEvent;
using adapters::SessionSpec;
using adapters::kAgentCount;

const char* SessionStateName(SessionState state) {
    switch (state) {
        case SessionState::Starting: return "STARTING";
        case SessionState::Running: return "RUNNING";
        case SessionState::Exited: return "EXITED";
        case SessionState::Cancelled: return "CANCELLED";
        case SessionState::Failed: return "FAILED";
        case SessionState::Unknown: return "UNKNOWN";
    }
    return "UNKNOWN";
}

namespace {

SessionState ParseAdapterState(const std::string& state) {
    if (state == "STARTING") return SessionState::Starting;
    if (state == "RUNNING") return SessionState::Running;
    if (state == "EXITED") return SessionState::Exited;
    if (state == "CANCELLED" || state == "STOPPED") return SessionState::Cancelled;
    if (state == "FAILED") return SessionState::Failed;
    return SessionState::Unknown;
}

}  // namespace

AgentGateway::AgentGateway(std::map<std::string, std::string> explicit_paths) {
    for (adapters::AgentKind kind : AllAgents()) {
        std::string path;
        auto it = explicit_paths.find(AgentId(kind));
        if (it != explicit_paths.end()) path = it->second;
        adapters_[kind] = std::make_unique<CliAdapter>(kind, std::move(path));
    }
}

adapters::CliAdapter* AgentGateway::AdapterFor(adapters::AgentKind kind) const {
    auto it = adapters_.find(kind);
    return it == adapters_.end() ? nullptr : it->second.get();
}

Status AgentGateway::SetExecutablePath(adapters::AgentKind kind, const std::string& path) {
    std::lock_guard<std::mutex> lock(mutex_);
    adapters_[kind] = std::make_unique<CliAdapter>(kind, path);
    return Status::Ok();
}

std::map<std::string, adapters::AdapterStatus> AgentGateway::ProbeAll() {
    std::map<std::string, adapters::AdapterStatus> result;
    for (adapters::AgentKind kind : AllAgents()) {
        auto status = Probe(kind);
        result[AgentId(kind)] = status;
    }
    return result;
}

adapters::AdapterStatus AgentGateway::Probe(adapters::AgentKind kind) {
    adapters::CliAdapter* adapter = AdapterFor(kind);
    if (!adapter) {
        adapters::AdapterStatus status;
        status.agent = kind;
        status.readiness = adapters::Readiness::Unsupported;
        status.reason = "no adapter instance";
        return status;
    }
    auto probed = adapter->Probe();
    if (!probed.ok()) {
        adapters::AdapterStatus status;
        status.agent = kind;
        status.readiness = adapters::Readiness::ProbeFailed;
        status.reason = probed.message();
        return status;
    }
    return probed.value();
}

adapters::AdapterStatus AgentGateway::AgentStatusOf(adapters::AgentKind kind) const {
    adapters::CliAdapter* adapter = AdapterFor(kind);
    if (!adapter) {
        adapters::AdapterStatus status;
        status.agent = kind;
        status.readiness = adapters::Readiness::Unsupported;
        status.reason = "no adapter instance";
        return status;
    }
    return adapter->last_status();
}

Expected<SessionRecord> AgentGateway::StartSession(adapters::AgentKind kind, const adapters::SessionSpec& spec,
                                                   const std::string& session_id) {
    adapters::CliAdapter* adapter = AdapterFor(kind);
    if (!adapter) {
        return Expected<SessionRecord>(
            mayasaba::Status::Error(ErrorCode::Unsupported, "no adapter for this agent"));
    }
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (sessions_.find(session_id) != sessions_.end()) {
            return Expected<SessionRecord>(
                mayasaba::Status::Error(ErrorCode::AlreadyExists, "session id already in use"));
        }
    }
    auto status = adapter->StartSession(spec, session_id);
    if (!status.ok()) return Expected<SessionRecord>(status);

    SessionRecord record;
    record.session_id = session_id;
    record.agent = kind;
    record.task_id = spec.task_id;
    record.attempt_id = spec.attempt_id;
    record.lease_id = spec.lease_id;
    record.lease_version = spec.lease_version;
    record.project_id = spec.project_id;
    record.state = SessionState::Running;
    record.started_at = NowUtcIso8601();
    {
        std::lock_guard<std::mutex> lock(mutex_);
        sessions_[session_id] = record;
    }
    return record;
}

Status AgentGateway::CancelSession(const std::string& session_id, std::string* observed_reason) {
    SessionRecord record;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = sessions_.find(session_id);
        if (it == sessions_.end()) return mayasaba::Status::Error(ErrorCode::NotFound, "no such session");
        record = it->second;
    }
    adapters::CliAdapter* adapter = AdapterFor(record.agent);
    if (!adapter) return mayasaba::Status::Error(ErrorCode::Internal, "adapter vanished");
    std::string reason;
    auto status = adapter->Cancel(session_id, &reason);
    if (!status.ok()) return status;
    adapters::CliAdapter::SessionSnapshot snapshot;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = sessions_.find(session_id);
        if (it != sessions_.end()) {
            if (adapter->Snapshot(session_id, &snapshot)) {
                it->second.state = ParseAdapterState(snapshot.state);
                it->second.detail = reason;
                it->second.ended_at = NowUtcIso8601();
            }
        }
    }
    if (observed_reason) *observed_reason = reason;
    return mayasaba::Status::Ok();
}

Status AgentGateway::StopSession(const std::string& session_id) {
    SessionRecord record;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = sessions_.find(session_id);
        if (it == sessions_.end()) return mayasaba::Status::Error(ErrorCode::NotFound, "no such session");
        record = it->second;
    }
    adapters::CliAdapter* adapter = AdapterFor(record.agent);
    if (!adapter) return mayasaba::Status::Error(ErrorCode::Internal, "adapter vanished");
    return adapter->Stop(session_id);
}

Status AgentGateway::PumpSession(const std::string& session_id) {
    SessionRecord record;
    adapters::CliAdapter* adapter = nullptr;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = sessions_.find(session_id);
        if (it == sessions_.end()) return mayasaba::Status::Error(ErrorCode::NotFound, "no such session");
        record = it->second;
        adapter = AdapterFor(record.agent);
    }
    if (!adapter) return mayasaba::Status::Error(ErrorCode::Internal, "adapter vanished");

    std::vector<adapters::SessionEvent> events = adapter->DrainEvents(session_id);
    adapters::CliAdapter::SessionSnapshot snapshot;
    if (adapter->Snapshot(session_id, &snapshot)) {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = sessions_.find(session_id);
        if (it != sessions_.end()) {
            it->second.state = ParseAdapterState(snapshot.state);
            it->second.event_count = snapshot.event_count;
            it->second.exit_code = snapshot.observation.exit_code;
            it->second.exit_code_observed = snapshot.observation.exit_code_observed;
            if (snapshot.state == "EXITED" || snapshot.state == "FAILED" ||
                snapshot.state == "CANCELLED" || snapshot.state == "UNKNOWN") {
                if (it->second.ended_at.empty()) it->second.ended_at = NowUtcIso8601();
                if (snapshot.state == "FAILED" && it->second.detail.empty()) {
                    it->second.detail = "session ended without parseable events";
                }
            }
            record = it->second;
        }
    }
    EventSink sink;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        sink = sink_;
    }
    if (sink && !events.empty()) sink(record, events);
    return mayasaba::Status::Ok();
}

Status AgentGateway::PumpAll() {
    std::vector<std::string> ids;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        for (const auto& [id, record] : sessions_) {
            (void)record;
            ids.push_back(id);
        }
    }
    for (const auto& id : ids) {
        auto status = PumpSession(id);
        if (!status.ok()) return status;
    }
    return mayasaba::Status::Ok();
}

std::vector<SessionRecord> AgentGateway::Sessions() const {
    std::lock_guard<std::mutex> lock(mutex_);
    std::vector<SessionRecord> out;
    out.reserve(sessions_.size());
    for (const auto& [id, record] : sessions_) {
        (void)id;
        out.push_back(record);
    }
    return out;
}

bool AgentGateway::Session(const std::string& session_id, SessionRecord* record) const {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = sessions_.find(session_id);
    if (it == sessions_.end()) return false;
    if (record) *record = it->second;
    return true;
}

void AgentGateway::SetEventSink(EventSink sink) {
    std::lock_guard<std::mutex> lock(mutex_);
    sink_ = std::move(sink);
}

AgentGateway::AvailabilityReport AgentGateway::Availability() const {
    AvailabilityReport report;
    for (adapters::AgentKind kind : AllAgents()) {
        auto status = AgentStatusOf(kind);
        const std::string id = AgentId(kind);
        if (status.readiness == adapters::Readiness::Ready) {
            report.ready_agents.push_back(id);
        } else {
            report.unavailable_agents.push_back(id);
            report.reasons[id] = status.reason.empty() ? adapters::ReadinessName(status.readiness)
                                                       : status.reason;
        }
    }
    report.all_ready = report.ready_agents.size() == static_cast<std::size_t>(kAgentCount);
    return report;
}

}  // namespace mayasaba::gateway
