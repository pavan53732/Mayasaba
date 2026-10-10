// Mayasaba Control Room — plain C++ Chat view-model.
//
// ChatViewModel is the UI's whole relationship with the product: it holds the controller
// handle, issues typed commands and queries, keeps a bounded in-memory projection of the
// timeline, and derives the small amount of presentation state the shell needs (attention
// notices, the operational-condition word).
//
// It contains no SQL, no process launch, no scheduling, no MCF parsing and no authority: every
// fallible call returns mayasaba::Status or Expected<T> straight from the controller facade.
#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "ControllerBridge.h"
#include "TimelineItemViewModel.h"

namespace mayasaba::app {

// One agent warning row, derived from control::AgentStatusView where needs_attention is true.
struct AgentNoticeViewModel {
    std::string agent_id;
    std::string display_name;
    std::string readiness;      // MISSING | UNSUPPORTED | PROBE_FAILED | CHECKING
    std::string reason;         // exact observed reason
    std::string effect;         // what this blocks right now
    std::string guidance;       // what to do outside Mayasaba
    std::string accessibility_name;
    bool can_locate = false;    // "Locate executable" only makes sense for MISSING
};

// One section of an expanded details sheet.
struct DetailsSectionViewModel {
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

// The dismissible details sheet content for one card.
struct DetailsViewModel {
    std::string card_id;
    std::string title;
    std::vector<DetailsSectionViewModel> sections;
    std::string raw_json;   // bounded machine-readable backing
    std::string accessibility_name;
};

AgentNoticeViewModel ProjectAgentNotice(const mayasaba::control::AgentStatusView& agent);

class ChatViewModel {
public:
    static constexpr std::size_t kMaxHistoryItems = 500;
    static constexpr std::size_t kMaxPendingItems = 200;
    static constexpr std::size_t kTimelinePageLimit = 200;

    void Attach(std::shared_ptr<mayasaba::control::Controller> controller) {
        controller_ = std::move(controller);
    }
    bool HasController() const { return controller_ != nullptr; }

    // --- Queries -------------------------------------------------------------------------
    mayasaba::Status RefreshProjectState();
    mayasaba::Status RefreshAgents();
    // Appends newly observed records to the pending buffer and the bounded history.
    mayasaba::Status RefreshTimeline();

    const mayasaba::control::ProjectState& ProjectState() const { return project_; }
    const std::vector<AgentNoticeViewModel>& AgentNotices() const { return notices_; }
    bool HasAttentionNotices() const { return !notices_.empty(); }
    // All three adapters, as observed. Used only by the explicitly opened diagnostics sheet.
    const std::vector<mayasaba::control::AgentStatusView>& AllAgents() const { return all_agents_; }

    std::vector<TimelineItemViewModel> TakePendingItems();
    bool HasPendingItems() const { return !pending_.empty(); }

    // Presentation-only projection over the visible history; not a new authoritative state.
    std::string DerivedOperationalCondition() const;

    // --- Commands ------------------------------------------------------------------------
    mayasaba::Expected<mayasaba::control::OpenFolderResult> OpenFolder(const std::string& display_path);
    mayasaba::Expected<mayasaba::control::SendReceipt> Send(
        const std::string& text,
        const std::vector<mayasaba::control::AttachmentRef>& attachments);
    mayasaba::Expected<mayasaba::control::Ack> CancelOperation(const std::string& operation_id);
    mayasaba::Expected<mayasaba::control::AgentStatusView> RetryAgentProbe(const std::string& agent_id);
    mayasaba::Expected<mayasaba::control::Ack> DismissCard(const std::string& card_id);
    mayasaba::Expected<mayasaba::control::Ack> SetAgentExecutablePath(const std::string& agent_id,
                                                                     const std::string& path);
    mayasaba::Expected<mayasaba::control::DetailsPayload> RequestDetails(const std::string& card_id);

    // Convenience: request details and flatten into the sheet view-model. Fails closed with the
    // controller's own reason when the card has nothing to show.
    mayasaba::Expected<DetailsViewModel> RequestDetailsView(const std::string& card_id);

private:
    std::shared_ptr<mayasaba::control::Controller> controller_;
    std::uint64_t last_sequence_ = 0;
    mayasaba::control::ProjectState project_{};
    std::vector<AgentNoticeViewModel> notices_;
    std::vector<mayasaba::control::AgentStatusView> all_agents_;
    std::vector<TimelineItemViewModel> pending_;
    std::vector<TimelineItemViewModel> history_;
};

}  // namespace mayasaba::app
