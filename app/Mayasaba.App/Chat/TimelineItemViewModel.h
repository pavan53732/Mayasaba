// Mayasaba Control Room — plain C++ timeline view-model and projection rules.
//
// This is presentation logic only: it turns a controller-owned control::TimelineItem into the
// strings, flags and visibility decisions the XAML DataTemplate needs. It performs no SQL, no
// process launch, no scheduling, and it never decides authority — it only decides how an
// already-authoritative record is described to the user.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "ControllerBridge.h"

namespace mayasaba::app {

// One action offered by a controller card.
struct CardActionViewModel {
    std::string action_id;
    std::string label;
    std::string command;      // controller command name, e.g. "RetryAgentProbe"
    std::string card_id;      // owning card, so the click handler needs no tree walk
    std::string request_json; // typed request payload, serialised as a bounded string
};

// One timeline entry, ready for binding.
struct TimelineItemViewModel {
    std::uint64_t sequence = 0;
    std::string card_id;
    std::string kind;
    std::string title;
    std::string body;
    std::string severity;   // info | attention | error | success
    std::string state;
    std::string timestamp;
    std::string author;     // "You", or the named agent for an agent message
    std::string kind_label; // short human label shown on controller cards
    std::string glyph;      // decorative Segoe Fluent Icons codepoint (never the only cue)
    std::string accessibility_name;
    bool has_details = false;
    bool dismissible = false;
    bool is_message = false;      // conversational bubble vs controller card
    bool is_provisional = false;  // streaming/proposed: explicitly labelled as not authoritative
    std::vector<CardActionViewModel> actions;
};

// The stable kind vocabulary declared by control::TimelineItem.kind.
inline constexpr const char* kKindUserMessage = "user_message";
inline constexpr const char* kKindAgentMessage = "agent_message";
inline constexpr const char* kKindStatus = "status";
inline constexpr const char* kKindWarning = "warning";
inline constexpr const char* kKindQuestion = "question";
inline constexpr const char* kKindProgress = "progress";
inline constexpr const char* kKindCouncilSummary = "council_summary";
inline constexpr const char* kKindRequirementsSummary = "requirements_summary";
inline constexpr const char* kKindDecision = "decision";
inline constexpr const char* kKindTask = "task";
inline constexpr const char* kKindValidation = "validation";
inline constexpr const char* kKindDelivery = "delivery";
inline constexpr const char* kKindError = "error";

// Maps a kind to the design-system card style key (Theme/Styles.xaml).
std::string StyleKeyForKind(const std::string& kind);

// Maps a kind to a short human label ("Council", "Validation", ...).
std::string KindLabelForKind(const std::string& kind);

// Maps a kind to a decorative glyph. Never the only carrier of meaning.
std::string GlyphForKind(const std::string& kind);

// "hermes" -> "Hermes", "kilo" -> "Kilo Code", "claude" -> "Claude Code". Agent attribution
// always names the agent; an unknown id is returned unchanged so nothing is invented.
std::string AgentDisplayNameFromId(const std::string& agent_id);

// Projects one controller record into the bindable view-model. Pure and total: an unknown kind
// or severity degrades to a quiet neutral card rather than dropping the record.
TimelineItemViewModel ProjectTimelineItem(const mayasaba::control::TimelineItem& item);

}  // namespace mayasaba::app
