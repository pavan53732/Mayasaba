// Mayasaba Control Room — timeline projection implementation (see TimelineItemViewModel.h).
#include "pch.h"

#include "TimelineItemViewModel.h"

#include <algorithm>
#include <cctype>

namespace mayasaba::app {

namespace {

std::string Lower(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return value;
}

// Reads a string field out of the typed per-kind payload without assuming its schema. Returns
// an empty string when the field is absent or not a string, so nothing is fabricated.
std::string PayloadString(const nlohmann::json& data, const char* key) {
    if (!data.is_object()) {
        return {};
    }
    auto it = data.find(key);
    if (it == data.end() || !it->is_string()) {
        return {};
    }
    return it->get<std::string>();
}

bool ContainsWord(const std::string& haystack, const char* needle) {
    return haystack.find(needle) != std::string::npos;
}

}  // namespace

std::string AgentDisplayNameFromId(const std::string& agent_id) {
    const std::string id = Lower(agent_id);
    if (id == "hermes") return "Hermes";
    if (id == "kilo" || id == "kilocode" || id == "kilo_code" || id == "kilo code") return "Kilo Code";
    if (id == "opencode" || id == "open_code" || id == "open code") return "OpenCode";
    return agent_id;
}

std::string StyleKeyForKind(const std::string& kind) {
    if (kind == kKindUserMessage) return "Card_UserMessage";
    if (kind == kKindAgentMessage) return "Card_AgentMessage";
    if (kind == kKindStatus) return "Card_Status";
    if (kind == kKindWarning) return "Card_Warning";
    if (kind == kKindQuestion) return "Card_Question";
    if (kind == kKindProgress) return "Card_Progress";
    if (kind == kKindCouncilSummary) return "Card_CouncilSummary";
    if (kind == kKindRequirementsSummary) return "Card_RequirementsSummary";
    if (kind == kKindDecision) return "Card_Decision";
    if (kind == kKindTask) return "Card_Task";
    if (kind == kKindValidation) return "Card_Validation";
    if (kind == kKindDelivery) return "Card_Delivery";
    if (kind == kKindError) return "Card_Error";
    return "Card_Default";
}

std::string KindLabelForKind(const std::string& kind) {
    if (kind == kKindUserMessage) return "You";
    if (kind == kKindAgentMessage) return "Agent";
    if (kind == kKindStatus) return "Status";
    if (kind == kKindWarning) return "Attention";
    if (kind == kKindQuestion) return "Question";
    if (kind == kKindProgress) return "In progress";
    if (kind == kKindCouncilSummary) return "Council";
    if (kind == kKindRequirementsSummary) return "Requirements";
    if (kind == kKindDecision) return "Decision";
    if (kind == kKindTask) return "Task";
    if (kind == kKindValidation) return "Validation";
    if (kind == kKindDelivery) return "Delivery";
    if (kind == kKindError) return "Error";
    return "Update";
}

std::string GlyphForKind(const std::string& kind) {
    if (kind == kKindStatus) return "\uE946";
    if (kind == kKindWarning) return "\uE7BA";
    if (kind == kKindQuestion) return "\uE897";
    if (kind == kKindProgress) return "\uE895";
    if (kind == kKindCouncilSummary) return "\uE8F1";
    if (kind == kKindRequirementsSummary) return "\uE8A5";
    if (kind == kKindDecision) return "\uE8FB";
    if (kind == kKindTask) return "\uE9D5";
    if (kind == kKindValidation) return "\uE73E";
    if (kind == kKindDelivery) return "\uE74E";
    if (kind == kKindError) return "\uEA39";
    return "\uE946";
}

TimelineItemViewModel ProjectTimelineItem(const mayasaba::control::TimelineItem& item) {
    TimelineItemViewModel vm;
    vm.sequence = item.sequence;
    vm.card_id = item.card_id;
    vm.kind = item.kind;
    vm.title = item.title;
    vm.body = item.body;
    vm.state = item.state;
    vm.timestamp = item.created_at;
    vm.has_details = item.has_details;
    vm.dismissible = item.dismissible;

    vm.severity = item.severity.empty() ? "info" : Lower(item.severity);
    if (vm.severity != "info" && vm.severity != "attention" && vm.severity != "error" &&
        vm.severity != "success") {
        vm.severity = "info";
    }

    vm.is_message = (item.kind == kKindUserMessage || item.kind == kKindAgentMessage);
    vm.kind_label = KindLabelForKind(item.kind);
    vm.glyph = vm.is_message ? std::string() : GlyphForKind(item.kind);

    // Author attribution. Agent messages always name Hermes, Kilo Code or OpenCode; a controller
    // card is never styled as a fourth agent.
    if (item.kind == kKindUserMessage) {
        vm.author = "You";
    } else if (item.kind == kKindAgentMessage) {
        std::string agent = PayloadString(item.data, "display_name");
        if (agent.empty()) agent = PayloadString(item.data, "agent");
        if (agent.empty()) agent = PayloadString(item.data, "agent_id");
        vm.author = agent.empty() ? std::string("Agent") : AgentDisplayNameFromId(agent);
        vm.kind_label = vm.author;
    } else {
        vm.author = "Mayasaba";
    }

    // Provisional means streaming or proposed, never authoritative (UI-UX.md section 2.4).
    const std::string state_lower = Lower(item.state);
    vm.is_provisional = (state_lower == "streaming" || state_lower == "provisional" ||
                         state_lower == "proposed");

    for (const auto& action : item.actions) {
        CardActionViewModel a;
        a.action_id = action.action_id;
        a.label = action.label;
        a.command = action.command;
        a.card_id = item.card_id;
        try {
            a.request_json = action.request.dump();
        } catch (...) {
            a.request_json = "{}";
        }
        vm.actions.push_back(std::move(a));
    }

    // A screen-reader sentence for the whole card: kind, title, state, and the one-sentence
    // consequence. Colour and glyph are never required to understand the entry.
    std::string name = vm.kind_label;
    name += vm.is_message ? " message. " : " card. ";
    if (!vm.title.empty()) {
        name += vm.title;
        name += ". ";
    }
    if (vm.is_provisional) {
        name += "Provisional, not yet authoritative. ";
    }
    if (!vm.state.empty()) {
        name += "State: " + vm.state + ". ";
    }
    if (!vm.body.empty() && vm.body != vm.title) {
        name += vm.body;
    }
    vm.accessibility_name = std::move(name);

    return vm;
}

}  // namespace mayasaba::app
