// Mayasaba Control Room — Chat view-model implementation.
#include "pch.h"

#include "ChatViewModel.h"

#include <algorithm>

namespace mayasaba::app {

using mayasaba::ErrorCode;
using mayasaba::Status;

namespace {

std::string Lower(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return value;
}

bool Contains(const std::string& haystack, const char* needle) {
    return haystack.find(needle) != std::string::npos;
}

// Readiness guidance is deliberately external: Mayasaba never installs, updates, authenticates
// or reconfigures a CLI, and never disguises a provider/task failure as an install problem.
void DescribeReadiness(const std::string& readiness, std::string& effect, std::string& guidance) {
    if (readiness == "MISSING") {
        effect = "Operations that require this CLI cannot start.";
        guidance = "Install it outside Mayasaba, or use Locate executable to point Mayasaba at the installed CLI.";
    } else if (readiness == "UNSUPPORTED") {
        effect = "Mayasaba will not run this CLI because the observed behaviour cannot be supported.";
        guidance = "Update the CLI outside Mayasaba, then Recheck.";
    } else if (readiness == "PROBE_FAILED") {
        effect = "A bounded readiness check failed, so this CLI is not usable right now.";
        guidance = "Confirm the CLI runs correctly outside Mayasaba, then Recheck.";
    } else if (readiness == "CHECKING") {
        effect = "Readiness is still being checked in the background.";
        guidance = "No action is needed yet.";
    } else {
        effect = "This CLI is not currently usable.";
        guidance = "Resolve the cause outside Mayasaba, then Recheck.";
    }
}

}  // namespace

AgentNoticeViewModel ProjectAgentNotice(const mayasaba::control::AgentStatusView& agent) {
    AgentNoticeViewModel vm;
    vm.agent_id = agent.agent_id;
    vm.display_name = agent.display_name.empty() ? AgentDisplayNameFromId(agent.agent_id)
                                                 : agent.display_name;
    vm.readiness = agent.readiness;
    vm.reason = agent.reason;
    vm.can_locate = (agent.readiness == "MISSING");
    DescribeReadiness(agent.readiness, vm.effect, vm.guidance);

    std::string name = vm.display_name + ", " + vm.readiness + ". ";
    if (!vm.reason.empty()) name += vm.reason + ". ";
    name += vm.effect + " " + vm.guidance;
    vm.accessibility_name = std::move(name);
    return vm;
}

Status ChatViewModel::RefreshProjectState() {
    if (!controller_) {
        return Status::Error(ErrorCode::NotReady, "controller is not attached");
    }
    auto state = controller_->QueryProjectState();
    if (!state.ok()) {
        return state.status();
    }
    project_ = state.value();
    return Status::Ok();
}

Status ChatViewModel::RefreshAgents() {
    if (!controller_) {
        return Status::Error(ErrorCode::NotReady, "controller is not attached");
    }
    auto agents = controller_->QueryAgents();
    if (!agents.ok()) {
        return agents.status();
    }
    notices_.clear();
    all_agents_ = agents.value();
    for (const auto& agent : agents.value()) {
        // Only missing or unusable CLIs occupy a warning row; READY agents never do.
        if (agent.needs_attention) {
            notices_.push_back(ProjectAgentNotice(agent));
        }
    }
    return Status::Ok();
}

Status ChatViewModel::RefreshTimeline() {
    if (!controller_) {
        return Status::Error(ErrorCode::NotReady, "controller is not attached");
    }
    mayasaba::control::TimelineQuery query;
    query.since_sequence = last_sequence_;
    query.limit = kTimelinePageLimit;

    auto page = controller_->QueryTimeline(query);
    if (!page.ok()) {
        return page.status();
    }

    for (const auto& item : page.value().items) {
        pending_.push_back(ProjectTimelineItem(item));
        history_.push_back(ProjectTimelineItem(item));
        if (item.sequence > last_sequence_) {
            last_sequence_ = item.sequence;
        }
    }
    if (page.value().next_sequence > last_sequence_) {
        last_sequence_ = page.value().next_sequence;
    }

    if (pending_.size() > kMaxPendingItems) {
        pending_.erase(pending_.begin(),
                       pending_.begin() + static_cast<std::ptrdiff_t>(pending_.size() - kMaxPendingItems));
    }
    if (history_.size() > kMaxHistoryItems) {
        history_.erase(history_.begin(),
                       history_.begin() + static_cast<std::ptrdiff_t>(history_.size() - kMaxHistoryItems));
    }
    return Status::Ok();
}

std::vector<TimelineItemViewModel> ChatViewModel::TakePendingItems() {
    std::vector<TimelineItemViewModel> drained;
    drained.swap(pending_);
    return drained;
}

std::string ChatViewModel::DerivedOperationalCondition() const {
    for (auto it = history_.rbegin(); it != history_.rend(); ++it) {
        const std::string state = Lower(it->state);
        if (state.empty()) {
            continue;
        }
        if (Contains(state, "cancelling")) return "Cancelling";
        if (Contains(state, "recovering")) return "Recovering";
        if (Contains(state, "blocked")) return "Blocked";
        if (Contains(state, "failed") || Contains(state, "error")) return "Failed";
        if (Contains(state, "stopped") || Contains(state, "cancelled")) return "Stopped";
        if (Contains(state, "waiting") || Contains(state, "pending")) return "Waiting";
        if (Contains(state, "running") || Contains(state, "active") || Contains(state, "progress")) {
            return "Running";
        }
        if (Contains(state, "idle") || Contains(state, "complete") || Contains(state, "done")) {
            return "Idle";
        }
    }
    return "Idle";
}

mayasaba::Expected<mayasaba::control::OpenFolderResult> ChatViewModel::OpenFolder(
    const std::string& display_path) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::OpenFolderResult>(ErrorCode::NotReady,
                                                                   "controller is not attached");
    }
    mayasaba::control::OpenFolderRequest request;
    request.display_path = display_path;
    return controller_->OpenFolder(request);
}

mayasaba::Expected<mayasaba::control::SendReceipt> ChatViewModel::Send(
    const std::string& text, const std::vector<mayasaba::control::AttachmentRef>& attachments) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::SendReceipt>(ErrorCode::NotReady,
                                                              "controller is not attached");
    }
    mayasaba::control::SendMessageRequest request;
    request.text = text;
    request.attachments = attachments;
    return controller_->SendMessage(request);
}

mayasaba::Expected<mayasaba::control::Ack> ChatViewModel::CancelOperation(const std::string& operation_id) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::Ack>(ErrorCode::NotReady, "controller is not attached");
    }
    mayasaba::control::CancelOperationRequest request;
    request.operation_id = operation_id;
    return controller_->CancelOperation(request);
}

mayasaba::Expected<mayasaba::control::AgentStatusView> ChatViewModel::RetryAgentProbe(
    const std::string& agent_id) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::AgentStatusView>(ErrorCode::NotReady,
                                                                  "controller is not attached");
    }
    mayasaba::control::RetryAgentProbeRequest request;
    request.agent_id = agent_id;
    return controller_->RetryAgentProbe(request);
}

mayasaba::Expected<mayasaba::control::Ack> ChatViewModel::DismissCard(const std::string& card_id) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::Ack>(ErrorCode::NotReady, "controller is not attached");
    }
    mayasaba::control::DismissCardRequest request;
    request.card_id = card_id;
    return controller_->DismissCard(request);
}

mayasaba::Expected<mayasaba::control::Ack> ChatViewModel::SetAgentExecutablePath(
    const std::string& agent_id, const std::string& path) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::Ack>(ErrorCode::NotReady, "controller is not attached");
    }
    // The path is the user's own "Locate executable" result. Mayasaba never invents one and
    // never writes model/provider configuration.
    mayasaba::control::SetAgentExecutablePathRequest request;
    request.agent_id = agent_id;
    request.path = path;
    return controller_->SetAgentExecutablePath(request);
}

mayasaba::Expected<mayasaba::control::DetailsPayload> ChatViewModel::RequestDetails(
    const std::string& card_id) {
    if (!controller_) {
        return mayasaba::Fail<mayasaba::control::DetailsPayload>(ErrorCode::NotReady,
                                                                 "controller is not attached");
    }
    mayasaba::control::RequestDetailsRequest request;
    request.card_id = card_id;
    return controller_->RequestDetails(request);
}

mayasaba::Expected<DetailsViewModel> ChatViewModel::RequestDetailsView(const std::string& card_id) {
    auto payload = RequestDetails(card_id);
    if (!payload.ok()) {
        return mayasaba::Expected<DetailsViewModel>(payload.status());
    }

    const auto& source = payload.value();
    DetailsViewModel vm;
    vm.card_id = source.card_id;
    vm.title = source.title.empty() ? std::string("Details") : source.title;
    for (const auto& section : source.sections) {
        DetailsSectionViewModel out;
        out.title = section.title;
        out.text = section.text;
        out.key_values = section.key_values;
        for (const auto& citation : section.citations) {
            out.citations.push_back(DetailsSectionViewModel::Citation{
                citation.source, citation.span, citation.text});
        }
        vm.sections.push_back(std::move(out));
    }
    try {
        vm.raw_json = source.raw.dump(2);
    } catch (...) {
        vm.raw_json.clear();
    }
    // Bound the machine-readable backing so a dense record cannot stall the sheet.
    static constexpr std::size_t kMaxRawChars = 8192;
    if (vm.raw_json.size() > kMaxRawChars) {
        vm.raw_json.resize(kMaxRawChars);
        vm.raw_json += "\n… (truncated for display)";
    }
    vm.accessibility_name = vm.title + " details";
    return mayasaba::Expected<DetailsViewModel>(std::move(vm));
}

}  // namespace mayasaba::app
