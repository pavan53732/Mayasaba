// Controller facade implementation (layer 2 application boundary for the Control Room UI).
//
// This is the only surface the WinUI layer talks to. Everything below it — storage, policy,
// task/DAG, council, context, evidence, validation, gateway, execution kernel — is owned by
// its own service; this class wires them, owns the background pump, and maintains the Chat
// timeline projection the UI renders. No SQL from the UI, no process handles, no agent channel.
#include "mayasaba/controller.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <map>
#include <memory>
#include <mutex>
#include <set>
#include <string>
#include <thread>
#include <vector>

#include "mayasaba/adapter.hpp"
#include "mayasaba/base.hpp"
#include "mayasaba/council.hpp"
#include "mayasaba/evidence.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/gateway.hpp"
#include "mayasaba/orchestrator.hpp"
#include "mayasaba/policy.hpp"
#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/tasks.hpp"
#include "mayasaba/validation.hpp"
#include "mayasaba/workspace.hpp"

// winuser.h's SendMessage macro must not collide with the typed command implementation.
#ifdef SendMessage
#undef SendMessage
#endif

using mayasaba::storage::Row;
using mayasaba::storage::SqlValue;

namespace mayasaba::control {
namespace {

Status EnsureTimelineSchema(storage::Store* store) {
    auto created = store->Exec(
        "CREATE TABLE IF NOT EXISTS control_timeline("
        "sequence INTEGER PRIMARY KEY AUTOINCREMENT,"
        "card_id TEXT NOT NULL, project_id TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL,"
        "title TEXT NOT NULL, body TEXT NOT NULL, severity TEXT NOT NULL DEFAULT 'info',"
        "state TEXT NOT NULL DEFAULT '', has_details INTEGER NOT NULL DEFAULT 0,"
        "dismissible INTEGER NOT NULL DEFAULT 0, actions TEXT NOT NULL DEFAULT '[]',"
        "data TEXT NOT NULL DEFAULT '{}', dismissed INTEGER NOT NULL DEFAULT 0,"
        "created_at TEXT NOT NULL)");
    if (!created.ok()) return created;
    return store->Exec(
        "CREATE INDEX IF NOT EXISTS idx_control_timeline_seq ON control_timeline(sequence)");
}

nlohmann::json ParseJsonOr(const std::string& text, nlohmann::json fallback) {
    auto parsed = nlohmann::json::parse(text, nullptr, false);
    return parsed.is_discarded() ? std::move(fallback) : parsed;
}

std::string JoinStrings(const std::vector<std::string>& values, const std::string& separator) {
    std::string out;
    for (std::size_t i = 0; i < values.size(); ++i) {
        if (i > 0) out += separator;
        out += values[i];
    }
    return out;
}

std::string Bounded(std::string text, std::size_t max_bytes) {
    if (text.size() <= max_bytes) return text;
    text.resize(max_bytes);
    text += "...";
    return text;
}

CardAction MakeAction(std::string action_id, std::string label, std::string command,
                      nlohmann::json request) {
    CardAction action;
    action.action_id = std::move(action_id);
    action.label = std::move(label);
    action.command = std::move(command);
    action.request = std::move(request);
    return action;
}

}  // namespace

// --- JSON helpers (stable, versioned; used by the UI and by contract tests) ---------------

nlohmann::json ToJson(const CardAction& action) {
    return {{"action_id", action.action_id}, {"label", action.label},
            {"command", action.command}, {"request", action.request}};
}

nlohmann::json ToJson(const TimelineItem& item) {
    nlohmann::json actions = nlohmann::json::array();
    for (const auto& action : item.actions) actions.push_back(ToJson(action));
    return {{"sequence", item.sequence}, {"card_id", item.card_id}, {"kind", item.kind},
            {"title", item.title}, {"body", item.body}, {"severity", item.severity},
            {"state", item.state}, {"has_details", item.has_details},
            {"dismissible", item.dismissible}, {"actions", actions}, {"data", item.data},
            {"created_at", item.created_at}};
}

nlohmann::json ToJson(const DetailsPayload& payload) {
    nlohmann::json sections = nlohmann::json::array();
    for (const auto& section : payload.sections) {
        nlohmann::json kv = nlohmann::json::array();
        for (const auto& pair : section.key_values) {
            kv.push_back({{"key", pair.first}, {"value", pair.second}});
        }
        nlohmann::json citations = nlohmann::json::array();
        for (const auto& citation : section.citations) {
            citations.push_back({{"source", citation.source}, {"span", citation.span},
                                 {"text", citation.text}});
        }
        sections.push_back({{"title", section.title}, {"text", section.text},
                            {"key_values", kv}, {"citations", citations}});
    }
    return {{"card_id", payload.card_id}, {"title", payload.title}, {"sections", sections},
            {"raw", payload.raw}};
}

nlohmann::json ToJson(const AgentStatusView& view) {
    return {{"agent_id", view.agent_id}, {"display_name", view.display_name},
            {"readiness", view.readiness}, {"reason", view.reason},
            {"needs_attention", view.needs_attention},
            {"executable_path", view.executable_path}};
}

nlohmann::json ToJson(const ProjectState& state) {
    return {{"bound", state.bound}, {"project_id", state.project_id},
            {"canonical_root", state.canonical_root}, {"display_path", state.display_path},
            {"phase", state.phase}, {"epoch", state.epoch}};
}

std::vector<std::string> CommandNames() {
    return {"OpenFolder", "SendMessage", "CancelOperation", "RetryAgentProbe",
            "DismissCard", "SetAgentExecutablePath", "RequestDetails"};
}

std::vector<std::string> QueryNames() {
    return {"QueryProjectState", "QueryTimeline", "QueryAgents"};
}

// --- Controller ---------------------------------------------------------------------------

class ControllerImpl final : public Controller {
public:
    explicit ControllerImpl(ControllerConfig config) : config_(std::move(config)) {}

    ~ControllerImpl() override { Shutdown(); }

    Status Initialize() override {
        auto dir = fs::EnsureDirectory(config_.storage_root);
        if (!dir.ok()) return dir;
        if (!config_.workspace_root.empty()) {
            auto ws = fs::EnsureDirectory(config_.workspace_root);
            if (!ws.ok()) return ws;
        }
        if (!config_.recovery_root.empty()) {
            auto rec = fs::EnsureDirectory(config_.recovery_root);
            if (!rec.ok()) return rec;
        }

        auto store = storage::Store::Open(fs::JoinPath(config_.storage_root, "mayasaba.db"));
        if (!store.ok()) return store.status();
        store_ = store.take();

        auto schema = EnsureTimelineSchema(store_.get());
        if (!schema.ok()) return schema;

        policy_ = std::make_unique<policy::Engine>(store_.get());
        tasks_ = std::make_unique<tasks::Engine>(store_.get());
        council_ = std::make_unique<council::Engine>();
        context_ = std::make_unique<context::Synchronizer>(store_.get(), std::string{});
        evidence_ = std::make_unique<evidence::Engine>(store_.get());
        validation_ = std::make_unique<validation::Engine>(store_.get(), evidence_.get());
        gateway_ = std::make_unique<gateway::AgentGateway>(config_.agent_executable_paths);

        // Apply persisted user-configured executable paths (Settings / Locate executable).
        for (adapters::AgentKind kind : adapters::AllAgents()) {
            if (auto saved = store_->GetSetting(std::string("agent.") + adapters::AgentId(kind) + ".path");
                saved.has_value() && !saved->empty()) {
                gateway_->SetExecutablePath(kind, *saved);
            }
        }

        app::Orchestrator::Services services;
        services.store = store_.get();
        services.policy = policy_.get();
        services.tasks = tasks_.get();
        services.council = council_.get();
        services.context = context_.get();
        services.evidence = evidence_.get();
        services.validation = validation_.get();
        services.gateway = gateway_.get();
        orchestrator_ = std::make_unique<app::Orchestrator>(services);

        gateway_->SetEventSink([this](const gateway::SessionRecord& session,
                                      const std::vector<adapters::SessionEvent>& events) {
            OnGatewayEvents(session, events);
        });

        // Startup reconciliation: pending publications are re-checked against actual bytes
        // and reported truthfully; never rolled back over newer user edits.
        auto pending = workspace::PendingPublications(store_.get());
        if (pending.ok()) {
            for (const auto& publication : *pending) {
                const std::string publication_id = publication.value("publication_id", "");
                const std::string project_id = publication.value("project_id", "");
                if (publication_id.empty()) continue;
                std::string root;
                auto rows = store_->Query(
                    "SELECT root_canonical FROM projects WHERE project_id = ?1",
                    {SqlValue::Text(project_id)});
                if (rows.ok() && !rows->empty()) root = rows->front().Text("root_canonical");
                if (root.empty()) continue;
                auto reconciled = workspace::Reconcile(publication_id, store_.get(), root);
                if (reconciled.ok() && reconciled->state != "PUBLISHED") {
                    AppendCard({.card_id = NewId("card"),
                                .kind = "status",
                                .title = "Publication reconciled",
                                .body = "Publication " + publication_id + " resolved to " +
                                        reconciled->state + ": " + reconciled->detail,
                                .severity = reconciled->state == "FAILED" ? "attention" : "info",
                                .has_details = true,
                                .data = {{"publication_id", publication_id},
                                         {"state", reconciled->state}}});
                }
            }
        }

        // Bounded, non-mutating readiness probes for the three CLIs; only missing/unusable
        // agents occupy warning rows.
        auto statuses = gateway_->ProbeAll();
        for (const auto& [agent_id, status] : statuses) {
            if (status.readiness == adapters::Readiness::Ready) continue;
            nlohmann::json data = {{"agent_id", agent_id},
                                   {"readiness", adapters::ReadinessName(status.readiness)},
                                   {"reason", status.reason}};
            nlohmann::json searched = nlohmann::json::array();
            for (const auto& candidate : status.observed_capabilities.value("searched", nlohmann::json::array())) {
                searched.push_back(candidate);
            }
            data["searched"] = searched;
            std::vector<CardAction> actions;
            actions.push_back(MakeAction("retry", "Retry", "RetryAgentProbe",
                                         {{"agent_id", agent_id}}));
            actions.push_back(MakeAction("locate", "Locate executable",
                                         "SetAgentExecutablePath", {{"agent_id", agent_id}}));
            AppendCard({.card_id = NewId("card"),
                        .kind = "warning",
                        .title = std::string(adapters::AgentName(
                                     *adapters::ParseAgentId(agent_id))) +
                                 " is not available",
                        .body = status.reason,
                        .severity = "attention",
                        .has_details = true,
                        .dismissible = true,
                        .actions = actions,
                        .data = data});
        }

        running_.store(true);
        pump_ = std::thread([this] { PumpLoop(); });
        return Status::Ok();
    }

    void Shutdown() override {
        if (running_.exchange(false) && pump_.joinable()) {
            pump_.join();
        }
    }

    Expected<OpenFolderResult> OpenFolder(const OpenFolderRequest& request) override {
        if (request.version != kControllerProtocolVersion) {
            return Fail<OpenFolderResult>(ErrorCode::InvalidArgument,
                                          "unsupported request version");
        }
        auto identity = orchestrator_->BindProjectRoot(request.display_path);
        if (!identity.ok()) {
            OpenFolderResult rejected;
            rejected.ok = false;
            rejected.detail = identity.message();
            AppendCard({.card_id = NewId("card"),
                        .kind = "error",
                        .title = "Folder rejected",
                        .body = identity.message(),
                        .severity = "error",
                        .data = {{"display_path", request.display_path}}});
        return rejected;
        }
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            current_project_ = identity->project_id;
        }
        auto phase = QueryProjectState();
        AppendCard({.card_id = NewId("card"),
                    .kind = "status",
                    .title = "Folder opened",
                    .body = identity->display_path,
                    .severity = "success",
                    .has_details = true,
                    .data = {{"project_id", identity->project_id},
                             {"canonical_root", identity->canonical_root},
                             {"epoch", identity->epoch}}});
        Notify();

        OpenFolderResult result;
        result.ok = true;
        result.project_id = identity->project_id;
        result.canonical_root = identity->canonical_root;
        result.display_path = identity->display_path;
        return result;
    }

    Expected<SendReceipt> SendMessage(const SendMessageRequest& request) override {
        if (request.version != kControllerProtocolVersion) {
            return Fail<SendReceipt>(ErrorCode::InvalidArgument,
                                     "unsupported request version");
        }
        std::string project_id;
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            project_id = current_project_;
        }
        if (project_id.empty()) {
            return Fail<SendReceipt>(ErrorCode::NotReady,
                                     "no project root is bound; open a folder first");
        }
        if (request.text.empty()) {
            return Fail<SendReceipt>(ErrorCode::InvalidArgument, "message text is empty");
        }

        // Every Send after binding persists exactly one immutable UserContribution, even if
        // no CLI is available.
        std::vector<std::string> attachment_refs;
        for (const auto& attachment : request.attachments) {
            nlohmann::json ref = {{"name", attachment.name}, {"path", attachment.path},
                                  {"size", attachment.size}, {"sha256", attachment.sha256}};
            attachment_refs.push_back(ref.dump());
        }
        auto persisted = orchestrator_->SubmitUserContribution(project_id, request.text,
                                                               attachment_refs);
        if (!persisted.ok()) return Fail<SendReceipt>(persisted.code(), persisted.message());

        const std::string card_id = persisted->contribution_id;
        AppendCard({.card_id = card_id,
                    .kind = "user_message",
                    .title = "You",
                    .body = request.text,
                    .severity = "info",
                    .has_details = !request.attachments.empty(),
                    .data = {{"contribution_id", persisted->contribution_id},
                             {"attachments", attachment_refs}}});
        Notify();

        // Agent-dependent work is gated here, never Chat: if no CLI is ready the message is
        // still persisted and the user is told exactly why no work started.
        auto availability = gateway_->Availability();
        if (availability.ready_agents.empty()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "warning",
                        .title = "No coding CLI is available",
                        .body = "Your message is saved. No agent work was started: " +
                                JoinStrings(availability.unavailable_agents, ", ") +
                                " not ready.",
                        .severity = "attention",
                        .dismissible = true,
                        .data = {{"unavailable", availability.unavailable_agents},
                                 {"reasons", availability.reasons}}});
            Notify();
            SendReceipt receipt;
            receipt.contribution_id = persisted->contribution_id;
            receipt.outcome = "PERSISTED";
            receipt.detail = "persisted; no CLI available, so no agent work was started";
            return receipt;
        }

        // Dispatch one leased task to a ready agent (Hermes preferred for the default slice).
        const std::string agent = [&]() {
            for (const std::string preferred : {"hermes", "kilo", "opencode"}) {
                if (std::find(availability.ready_agents.begin(), availability.ready_agents.end(),
                              preferred) != availability.ready_agents.end()) {
                    return preferred;
                }
            }
            return availability.ready_agents.front();
        }();

        tasks::TaskContract contract;
        contract.project_id = project_id;
        contract.contribution_id = persisted->contribution_id;
        contract.objective = request.text;
        tasks::AcceptanceCriterion criterion;
        criterion.criterion_id = NewId("crit");
        criterion.expectation = "The published result satisfies the user request";
        criterion.oracle_class = "user_acceptance";
        criterion.oracle_spec = "User reviews the published changes in Chat";
        criterion.blocking = true;
        contract.criteria.push_back(criterion);

        auto task = orchestrator_->ScheduleTask(project_id, "User request", contract);
        if (!task.ok()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "error",
                        .title = "Work request rejected",
                        .body = task.message(),
                        .severity = "error"});
            Notify();
            SendReceipt receipt;
            receipt.contribution_id = persisted->contribution_id;
            receipt.outcome = "BLOCKED";
            receipt.detail = task.message();
            return receipt;
        }
        auto lease = orchestrator_->LeaseTask(task->task_id, agent);
        if (!lease.ok()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "error",
                        .title = "Lease refused",
                        .body = lease.message(),
                        .severity = "error",
                        .data = {{"task_id", task->task_id}}});
            Notify();
            SendReceipt receipt;
            receipt.contribution_id = persisted->contribution_id;
            receipt.outcome = "BLOCKED";
            receipt.detail = lease.message();
            return receipt;
        }
        auto session = orchestrator_->StartTaskExecution(task->task_id, lease->attempt_id, agent);
        const std::string task_card = NewId("card");
        if (!session.ok()) {
            AppendCard({.card_id = task_card,
                        .kind = "task",
                        .title = "Task could not start",
                        .body = session.message(),
                        .severity = "error",
                        .state = "BLOCKED",
                        .has_details = true,
                        .data = {{"task_id", task->task_id}, {"agent", agent}}});
        } else {
            {
                std::lock_guard<std::mutex> lock(cards_mutex_);
                task_cards_[task_card] = *session;
            }
            AppendCard({.card_id = task_card,
                        .kind = "task",
                        .title = "Working on your request",
                        .body = std::string(adapters::AgentName(*adapters::ParseAgentId(agent))) +
                                " is working in an isolated staging copy.",
                        .severity = "info",
                        .state = "RUNNING",
                        .has_details = true,
                        .actions = {{{"cancel", "Cancel", "CancelOperation",
                                      {{"operation_id", task_card}}}}},
                        .data = {{"task_id", task->task_id}, {"agent", agent},
                                 {"session_id", *session},
                                 {"lease_version", lease->version}}});
        }
        Notify();
        SendReceipt receipt;
        receipt.contribution_id = persisted->contribution_id;
        receipt.outcome = "PERSISTED";
        receipt.detail = session.ok() ? "persisted; agent work started"
                                      : "persisted; agent work could not start";
        return receipt;
    }

    Expected<Ack> CancelOperation(const CancelOperationRequest& request) override {
        if (request.version != kControllerProtocolVersion) {
            return Fail<Ack>(ErrorCode::InvalidArgument, "unsupported request version");
        }
        std::string session_id;
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            const auto it = task_cards_.find(request.operation_id);
            if (it != task_cards_.end()) session_id = it->second;
        }
        if (session_id.empty()) {
            return Fail<Ack>(ErrorCode::NotFound,
                             "no active operation for id " + request.operation_id);
        }
        std::string reason;
        auto cancelled = gateway_->CancelSession(session_id, &reason);
        if (!cancelled.ok()) return Fail<Ack>(cancelled.code(), cancelled.message());
        AppendCard({.card_id = request.operation_id,
                    .kind = "task",
                    .title = "Cancelled",
                    .body = reason.empty() ? "operation cancelled by the user" : reason,
                    .severity = "info",
                    .state = "CANCELLED",
                    .has_details = true,
                    .data = {{"session_id", session_id}}});
        Notify();
        Ack ack;
        ack.ok = true;
        ack.detail = reason;
        return ack;
    }

    Expected<AgentStatusView> RetryAgentProbe(const RetryAgentProbeRequest& request) override {
        auto kind = adapters::ParseAgentId(request.agent_id);
        if (!kind.has_value()) {
            return Fail<AgentStatusView>(ErrorCode::InvalidArgument,
                                         "unknown agent id: " + request.agent_id);
        }
        auto status = gateway_->Probe(*kind);
        AppendCard({.card_id = NewId("card"),
                    .kind = "status",
                    .title = std::string(adapters::AgentName(*kind)) + " re-probed",
                    .body = status.readiness == adapters::Readiness::Ready
                                ? "ready"
                                : status.reason,
                    .severity = status.readiness == adapters::Readiness::Ready ? "success"
                                                                               : "attention",
                    .has_details = true,
                    .data = {{"agent_id", request.agent_id},
                             {"readiness", adapters::ReadinessName(status.readiness)},
                             {"reason", status.reason}}});
        Notify();
        return MakeAgentView(*kind, status);
    }

    Expected<Ack> DismissCard(const DismissCardRequest& request) override {
        auto updated = store_->Exec(
            "UPDATE control_timeline SET dismissed = 1 WHERE card_id = ?1",
            {SqlValue::Text(request.card_id)});
        if (!updated.ok()) return Fail<Ack>(updated.code(), updated.message());
        Notify();
        Ack ack;
        ack.ok = true;
        ack.detail = "dismissed";
        return ack;
    }

    Expected<Ack> SetAgentExecutablePath(const SetAgentExecutablePathRequest& request) override {
        auto kind = adapters::ParseAgentId(request.agent_id);
        if (!kind.has_value()) {
            return Fail<Ack>(ErrorCode::InvalidArgument,
                             "unknown agent id: " + request.agent_id);
        }
        if (request.path.empty()) {
            return Fail<Ack>(ErrorCode::InvalidArgument, "executable path is empty");
        }
        if (auto exists = fs::IdentifyPath(request.path); !exists.ok() || !exists->exists) {
            return Fail<Ack>(ErrorCode::NotFound,
                             "no file exists at the provided path: " + request.path);
        }
        auto saved = store_->SetSetting(std::string("agent.") + request.agent_id + ".path",
                                        request.path);
        if (!saved.ok()) return Fail<Ack>(saved.code(), saved.message());
        gateway_->SetExecutablePath(*kind, request.path);
        auto status = gateway_->Probe(*kind);
        AppendCard({.card_id = NewId("card"),
                    .kind = "status",
                    .title = std::string(adapters::AgentName(*kind)) + " executable updated",
                    .body = status.readiness == adapters::Readiness::Ready
                                ? "ready with the configured executable"
                                : status.reason,
                    .severity = status.readiness == adapters::Readiness::Ready ? "success"
                                                                               : "attention",
                    .has_details = true,
                    .data = {{"agent_id", request.agent_id},
                             {"path", request.path},
                             {"readiness", adapters::ReadinessName(status.readiness)}}});
        Notify();
        Ack ack;
        ack.ok = true;
        ack.detail = "saved and re-probed";
        return ack;
    }

    Expected<DetailsPayload> RequestDetails(const RequestDetailsRequest& request) override {
        auto rows = store_->Query(
            "SELECT kind, title, body, severity, state, data, created_at FROM control_timeline "
            "WHERE card_id = ?1 ORDER BY sequence DESC LIMIT 1",
            {SqlValue::Text(request.card_id)});
        if (!rows.ok()) return Fail<DetailsPayload>(rows.code(), rows.message());
        if (rows->empty()) {
            return Fail<DetailsPayload>(ErrorCode::NotFound,
                                        "no card with id " + request.card_id);
        }
        const Row& row = rows->front();
        DetailsPayload payload;
        payload.card_id = request.card_id;
        payload.title = row.Text("title");
        payload.raw = ParseJsonOr(row.Text("data"), nlohmann::json::object());

        DetailsSection summary;
        summary.title = "Summary";
        summary.text = row.Text("body");
        summary.key_values.push_back({"kind", row.Text("kind")});
        summary.key_values.push_back({"severity", row.Text("severity")});
        if (!row.Text("state").empty()) summary.key_values.push_back({"state", row.Text("state")});
        summary.key_values.push_back({"created_at", row.Text("created_at")});
        payload.sections.push_back(summary);

        const std::string kind = row.Text("kind");
        const std::string task_id = payload.raw.value("task_id", "");
        if (!task_id.empty()) {
            if (auto task = tasks_->GetTask(task_id); task.ok()) {
                DetailsSection task_section;
                task_section.title = "Task";
                task_section.text = task->contract.objective;
                task_section.key_values.push_back({"task_id", task->task_id});
                task_section.key_values.push_back({"state", tasks::TaskStateName(task->state)});
                task_section.key_values.push_back({"agent", task->assigned_agent});
                for (const auto& criterion : task->contract.criteria) {
                    task_section.key_values.push_back(
                        {"criterion " + criterion.criterion_id, criterion.expectation});
                }
                if (auto attempts = tasks_->AttemptsFor(task_id); attempts.ok()) {
                    for (const auto& attempt : *attempts) {
                        task_section.key_values.push_back(
                            {"attempt " + attempt.attempt_id,
                             attempt.status + " (" + attempt.agent + ")"});
                    }
                }
                payload.sections.push_back(task_section);
            }
        }
        if (kind == "delivery" || payload.raw.contains("publication_id")) {
            const std::string publication_id = payload.raw.value("publication_id", "");
            if (!publication_id.empty()) {
                auto files = store_->Query(
                    "SELECT rel_path, operation, state FROM publication_files "
                    "WHERE publication_id = ?1 ORDER BY rel_path",
                    {SqlValue::Text(publication_id)});
                DetailsSection files_section;
                files_section.title = "Published files";
                if (files.ok()) {
                    for (const auto& file : *files) {
                        files_section.key_values.push_back(
                            {file.Text("operation"), file.Text("rel_path")});
                    }
                }
                payload.sections.push_back(files_section);
            }
        }
        if (kind == "council_summary" || payload.raw.contains("point_id")) {
            const std::string point_id = payload.raw.value("point_id", "");
            if (!point_id.empty()) {
                if (auto state = council_->PointStateOf(point_id); state.ok()) {
                    DetailsSection council_section;
                    council_section.title = "Council";
                    council_section.text = state->point.topic;
                    council_section.key_values.push_back(
                        {"rounds", std::to_string(state->rounds.size())});
                    council_section.key_values.push_back(
                        {"proposals", std::to_string(state->proposals.size())});
                    council_section.key_values.push_back(
                        {"critiques", std::to_string(state->critiques.size())});
                    council_section.key_values.push_back(
                        {"syntheses", std::to_string(state->syntheses.size())});
                    council_section.key_values.push_back({"rationale", state->rationale});
                    payload.sections.push_back(council_section);
                }
            }
        }
        if (kind == "validation" || payload.raw.contains("criterion_id")) {
            const std::string criterion_id = payload.raw.value("criterion_id", "");
            if (!criterion_id.empty()) {
                if (auto records = evidence_->Records(criterion_id); records.ok()) {
                    DetailsSection evidence_section;
                    evidence_section.title = "Evidence";
                    for (const auto& record : *records) {
                        evidence_section.key_values.push_back(
                            {record.check_id + "@" + record.check_version,
                             record.outcome + " - " + record.artifact.provenance});
                    }
                    payload.sections.push_back(evidence_section);
                }
            }
        }
        return payload;
    }

    Expected<ProjectState> QueryProjectState() override {
        ProjectState state;
        std::string project_id;
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            project_id = current_project_;
        }
        if (project_id.empty()) return state;   // bound == false; Send stays disabled
        auto rows = store_->Query(
            "SELECT project_id, root_canonical, root_path, epoch, lifecycle_phase FROM projects "
            "WHERE project_id = ?1",
            {SqlValue::Text(project_id)});
        if (!rows.ok()) return Fail<ProjectState>(rows.code(), rows.message());
        if (rows->empty()) return state;
        const Row& row = rows->front();
        state.bound = true;
        state.project_id = row.Text("project_id");
        state.canonical_root = row.Text("root_canonical");
        state.display_path = row.Text("root_path");
        state.epoch = row.Int("epoch");
        state.phase = row.Text("lifecycle_phase");
        return state;
    }

    Expected<TimelinePage> QueryTimeline(const TimelineQuery& query) override {
        const std::size_t limit = std::min<std::size_t>(query.limit == 0 ? 200 : query.limit, 500);
        auto rows = store_->Query(
            "SELECT sequence, card_id, kind, title, body, severity, state, has_details,"
            " dismissible, actions, data, created_at FROM control_timeline "
            "WHERE sequence > ?1 AND dismissed = 0 ORDER BY sequence LIMIT ?2",
            {SqlValue::Int(static_cast<std::int64_t>(query.since_sequence)),
             SqlValue::Int(static_cast<std::int64_t>(limit + 1))});
        if (!rows.ok()) return Fail<TimelinePage>(rows.code(), rows.message());
        TimelinePage page;
        for (const auto& row : *rows) {
            if (page.items.size() == limit) {
                page.more = true;
                break;
            }
            TimelineItem item;
            item.sequence = static_cast<std::uint64_t>(row.Int("sequence"));
            item.card_id = row.Text("card_id");
            item.kind = row.Text("kind");
            item.title = row.Text("title");
            item.body = row.Text("body");
            item.severity = row.Text("severity");
            item.state = row.Text("state");
            item.has_details = row.Int("has_details") != 0;
            item.dismissible = row.Int("dismissible") != 0;
            item.data = ParseJsonOr(row.Text("data"), nlohmann::json::object());
            item.created_at = row.Text("created_at");
            const auto actions = ParseJsonOr(row.Text("actions"), nlohmann::json::array());
            for (const auto& action : actions) {
                CardAction parsed;
                parsed.action_id = action.value("action_id", "");
                parsed.label = action.value("label", "");
                parsed.command = action.value("command", "");
                parsed.request = action.value("request", nlohmann::json::object());
                item.actions.push_back(parsed);
            }
            page.next_sequence = item.sequence;
            page.items.push_back(std::move(item));
        }
        return page;
    }

    Expected<std::vector<AgentStatusView>> QueryAgents() override {
        std::vector<AgentStatusView> views;
        for (adapters::AgentKind kind : adapters::AllAgents()) {
            views.push_back(MakeAgentView(kind, gateway_->AgentStatusOf(kind)));
        }
        return views;
    }

    void SetNoticeCallback(NoticeCallback callback) override {
        std::lock_guard<std::mutex> lock(cards_mutex_);
        notice_ = std::move(callback);
    }

private:
    struct CardSpec {
        std::string card_id;
        std::string kind;
        std::string title;
        std::string body;
        std::string severity = "info";
        std::string state;
        bool has_details = false;
        bool dismissible = false;
        std::vector<CardAction> actions;
        nlohmann::json data = nlohmann::json::object();
    };

    void AppendCard(const CardSpec& spec) {
        nlohmann::json actions = nlohmann::json::array();
        for (const auto& action : spec.actions) actions.push_back(ToJson(action));
        std::string project_id;
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            project_id = current_project_;
        }
        store_->Exec(
            "INSERT INTO control_timeline(card_id, project_id, kind, title, body, severity,"
            " state, has_details, dismissible, actions, data, created_at)"
            " VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            {SqlValue::Text(spec.card_id), SqlValue::Text(project_id),
             SqlValue::Text(spec.kind), SqlValue::Text(spec.title), SqlValue::Text(spec.body),
             SqlValue::Text(spec.severity), SqlValue::Text(spec.state),
             SqlValue::Int(spec.has_details ? 1 : 0), SqlValue::Int(spec.dismissible ? 1 : 0),
             SqlValue::Text(actions.dump()), SqlValue::Text(spec.data.dump()),
             SqlValue::Text(NowUtcIso8601())});
    }

    void Notify() {
        NoticeCallback callback;
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            callback = notice_;
        }
        if (callback) callback();
    }

    AgentStatusView MakeAgentView(adapters::AgentKind kind,
                                  const adapters::AdapterStatus& status) const {
        AgentStatusView view;
        view.agent_id = adapters::AgentId(kind);
        view.display_name = adapters::AgentName(kind);
        view.readiness = adapters::ReadinessName(status.readiness);
        view.reason = status.reason;
        view.needs_attention = status.readiness != adapters::Readiness::Ready &&
                               status.readiness != adapters::Readiness::Checking;
        view.executable_path = status.executable_path;
        return view;
    }

    void OnGatewayEvents(const gateway::SessionRecord& session,
                         const std::vector<adapters::SessionEvent>& events) {
        for (const auto& event : events) {
            if (event.kind == "message" && !event.text.empty()) {
                AppendCard({.card_id = NewId("card"),
                            .kind = "agent_message",
                            .title = std::string(adapters::AgentName(session.agent)),
                            .body = Bounded(event.text, 8000),
                            .severity = "info",
                            .has_details = true,
                            .data = {{"session_id", session.session_id},
                                     {"task_id", session.task_id}}});
                Notify();
            } else if (event.kind == "error" && !event.text.empty()) {
                AppendCard({.card_id = NewId("card"),
                            .kind = "error",
                            .title = std::string(adapters::AgentName(session.agent)) +
                                     " reported an error",
                            .body = Bounded(event.text, 4000),
                            .severity = "error",
                            .has_details = true,
                            .data = {{"session_id", session.session_id},
                                     {"task_id", session.task_id}}});
                Notify();
            }
        }
    }

    void PumpLoop() {
        while (running_.load()) {
            gateway_->PumpAll();
            DriveFinishedSessions();
            ExpireLeases();
            std::this_thread::sleep_for(std::chrono::milliseconds(400));
        }
    }

    void DriveFinishedSessions() {
        for (const auto& record : gateway_->Sessions()) {
            if (record.state == gateway::SessionState::Running ||
                record.state == gateway::SessionState::Starting) {
                continue;
            }
            bool first_time = false;
            {
                std::lock_guard<std::mutex> lock(cards_mutex_);
                first_time = handled_sessions_.insert(record.session_id).second;
            }
            if (!first_time) continue;

            const bool success = record.state == gateway::SessionState::Exited &&
                                 record.exit_code == 0;
            std::string detail;
            if (record.state == gateway::SessionState::Cancelled) detail = "cancelled by user";
            else if (record.state == gateway::SessionState::Failed) detail = record.detail;
            else if (!success) detail = "session exited without success";

            auto completed = orchestrator_->OnAgentSessionCompleted(record.session_id, success,
                                                                    detail);
            if (!completed.ok()) {
                AppendCard({.card_id = NewId("card"),
                            .kind = "error",
                            .title = "Completion routing failed",
                            .body = completed.message(),
                            .severity = "error",
                            .data = {{"session_id", record.session_id},
                                     {"task_id", record.task_id}}});
                Notify();
                continue;
            }
            if (record.task_id.empty()) continue;

            if (!success) {
                AppendCard({.card_id = NewId("card"),
                            .kind = "task",
                            .title = "Task failed",
                            .body = detail.empty() ? "the agent session did not succeed" : detail,
                            .severity = "error",
                            .state = "FAILED",
                            .has_details = true,
                            .data = {{"task_id", record.task_id},
                                     {"session_id", record.session_id}}});
                Notify();
                continue;
            }
            DriveIntegration(record.task_id);
        }
    }

    void DriveIntegration(const std::string& task_id) {
        auto candidate = orchestrator_->PrepareIntegration(task_id);
        if (!candidate.ok()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "error",
                        .title = "Integration failed",
                        .body = candidate.message(),
                        .severity = "error",
                        .data = {{"task_id", task_id}}});
            Notify();
            return;
        }
        if (!candidate->conflicts.empty()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "delivery",
                        .title = "Publication needs your review",
                        .body = "Files changed in your folder since the agent started; nothing "
                                "was overwritten.",
                        .severity = "attention",
                        .state = "CONFLICT",
                        .has_details = true,
                        .data = {{"task_id", task_id}, {"conflicts", candidate->conflicts}}});
            Notify();
            return;
        }

        if (candidate->changes.empty()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "delivery",
                        .title = "No file changes were produced",
                        .body = "The agent session finished without modifying the staged copy.",
                        .severity = "info",
                        .state = "PUBLISHED",
                        .has_details = true,
                        .data = {{"task_id", task_id}, {"changes", nlohmann::json::array()}}});
            Notify();
            DriveValidation(task_id);
            return;
        }

        auto published = orchestrator_->PublishIntegration(
            task_id, config_.recovery_root.empty() ? std::string{} : config_.recovery_root);
        if (!published.ok()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "error",
                        .title = "Publication failed",
                        .body = published.message(),
                        .severity = "error",
                        .data = {{"task_id", task_id}}});
            Notify();
            return;
        }
        if (published->state != "PUBLISHED") {
            AppendCard({.card_id = NewId("card"),
                        .kind = "delivery",
                        .title = "Publication " + published->state,
                        .body = published->detail,
                        .severity = "attention",
                        .state = published->state,
                        .has_details = true,
                        .data = {{"task_id", task_id},
                                 {"publication_id", published->publication_id},
                                 {"failed_paths", published->failed_paths}}});
            Notify();
            return;
        }

        AppendCard({.card_id = NewId("card"),
                    .kind = "delivery",
                    .title = "Changes published",
                    .body = std::to_string(published->applied_paths.size()) +
                            " file(s) published to your folder.",
                    .severity = "success",
                    .state = "PUBLISHED",
                    .has_details = true,
                    .data = {{"task_id", task_id},
                             {"publication_id", published->publication_id},
                             {"applied_paths", published->applied_paths}}});
        Notify();
        DriveValidation(task_id);
    }

    void DriveValidation(const std::string& task_id) {
        auto val_res = orchestrator_->ValidateTask(task_id);
        if (!val_res.ok()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "validation",
                        .title = "Validation failed",
                        .body = "Validation could not be completed: " + val_res.message(),
                        .severity = "attention",
                        .state = "FAIL",
                        .has_details = true,
                        .data = {{"task_id", task_id}, {"error", val_res.message()}}});
            Notify();
            return;
        }

        const auto& verdicts = *val_res;
        std::string rationale;
        bool all_pass = !verdicts.empty();
        bool any_blocking_failed = false;
        for (const auto& v : verdicts) {
            if (!rationale.empty()) rationale += "; ";
            rationale += v.criterion_id + "=" + validation::VerdictName(v.verdict);
            if (v.verdict == validation::Verdict::Fail) any_blocking_failed = true;
            if (v.verdict != validation::Verdict::Pass && v.verdict != validation::Verdict::NotApplicable) {
                all_pass = false;
            }
        }

        if (all_pass) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "validation",
                        .title = "Task completed",
                        .body = "All blocking acceptance criteria passed: " + rationale,
                        .severity = "success",
                        .state = "PASS",
                        .has_details = true,
                        .data = {{"task_id", task_id}, {"rationale", rationale}}});
        } else {
            // A user_acceptance criterion passes only when the user accepts; the task stays
            // open for review rather than being marked complete by the controller.
            AppendCard({.card_id = NewId("card"),
                        .kind = "validation",
                        .title = any_blocking_failed ? "Validation failed" : "Waiting for your review",
                        .body = verdicts.empty()
                                    ? "No acceptance criteria specified; task cannot be certified."
                                    : "Changes are published. Acceptance: " + rationale,
                        .severity = "attention",
                        .state = any_blocking_failed ? "FAIL" : "INCONCLUSIVE",
                        .has_details = true,
                        .data = {{"task_id", task_id}, {"rationale", rationale}}});
        }
        Notify();
    }

    void ExpireLeases() {
        std::string project_id;
        {
            std::lock_guard<std::mutex> lock(cards_mutex_);
            project_id = current_project_;
        }
        if (project_id.empty()) return;
        auto expired = tasks_->ExpireStaleLeases(project_id);
        if (expired.ok() && !expired->empty()) {
            AppendCard({.card_id = NewId("card"),
                        .kind = "status",
                        .title = "Expired leases reclaimed",
                        .body = std::to_string(expired->size()) +
                                " stale lease(s) returned to READY.",
                        .severity = "attention",
                        .data = {{"task_ids", *expired}}});
            Notify();
        }
    }

    ControllerConfig config_;
    std::unique_ptr<storage::Store> store_;
    std::unique_ptr<policy::Engine> policy_;
    std::unique_ptr<tasks::Engine> tasks_;
    std::unique_ptr<council::Engine> council_;
    std::unique_ptr<context::Synchronizer> context_;
    std::unique_ptr<evidence::Engine> evidence_;
    std::unique_ptr<validation::Engine> validation_;
    std::unique_ptr<gateway::AgentGateway> gateway_;
    std::unique_ptr<app::Orchestrator> orchestrator_;

    std::thread pump_;
    std::atomic<bool> running_{false};

    std::mutex cards_mutex_;
    NoticeCallback notice_;
    std::string current_project_;
    std::map<std::string, std::string> task_cards_;    // card_id -> session_id
    std::set<std::string> handled_sessions_;
};

Expected<std::unique_ptr<Controller>> Controller::Create(const ControllerConfig& config) {
    if (config.storage_root.empty()) {
        return Fail<std::unique_ptr<Controller>>(ErrorCode::InvalidArgument,
                                                 "storage_root is required");
    }
    return std::unique_ptr<Controller>(new ControllerImpl(config));
}

}  // namespace mayasaba::control
