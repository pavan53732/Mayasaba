// Mayasaba Control Room — bindable WinRT item types and their factories.
//
// Each struct below is the C++/WinRT implementation of a runtimeclass declared in Project.idl.
// The matching generated *.g.cpp files are included at the bottom of this translation unit, as
// C++/WinRT requires for a project that declares its own runtimeclasses.
#include "pch.h"

#include "BindableItems.h"

#include "CardActionView.g.h"
#include "TimelineCard.g.h"
#include "AgentNotice.g.h"
#include "AttachmentChip.g.h"

#include <cstdint>
#include <string>

using winrt::Microsoft::UI::Xaml::Visibility;

namespace winrt::Mayasaba::App::implementation {

// ---------------------------------------------------------------------------------------------
// CardActionView
// ---------------------------------------------------------------------------------------------
struct CardActionView : CardActionViewT<CardActionView> {
    CardActionView() = default;

    hstring ActionId() const { return action_id_; }
    void ActionId(hstring const& value) { action_id_ = value; }
    hstring Label() const { return label_; }
    void Label(hstring const& value) { label_ = value; }
    hstring Command() const { return command_; }
    void Command(hstring const& value) { command_ = value; }
    hstring CardId() const { return card_id_; }
    void CardId(hstring const& value) { card_id_ = value; }
    hstring RequestJson() const { return request_json_; }
    void RequestJson(hstring const& value) { request_json_ = value; }

private:
    hstring action_id_{};
    hstring label_{};
    hstring command_{};
    hstring card_id_{};
    hstring request_json_{};
};

// ---------------------------------------------------------------------------------------------
// TimelineCard
// ---------------------------------------------------------------------------------------------
struct TimelineCard : TimelineCardT<TimelineCard> {
    TimelineCard() = default;

    std::uint64_t Sequence() const { return sequence_; }
    void Sequence(std::uint64_t value) { sequence_ = value; }
    hstring CardId() const { return card_id_; }
    void CardId(hstring const& value) { card_id_ = value; }
    hstring Kind() const { return kind_; }
    void Kind(hstring const& value) { kind_ = value; }
    hstring Title() const { return title_; }
    void Title(hstring const& value) { title_ = value; }
    hstring Body() const { return body_; }
    void Body(hstring const& value) { body_ = value; }
    hstring Severity() const { return severity_; }
    void Severity(hstring const& value) { severity_ = value; }
    hstring State() const { return state_; }
    void State(hstring const& value) { state_ = value; }
    hstring Timestamp() const { return timestamp_; }
    void Timestamp(hstring const& value) { timestamp_ = value; }
    hstring Author() const { return author_; }
    void Author(hstring const& value) { author_ = value; }
    hstring KindLabel() const { return kind_label_; }
    void KindLabel(hstring const& value) { kind_label_ = value; }
    hstring Glyph() const { return glyph_; }
    void Glyph(hstring const& value) { glyph_ = value; }
    hstring AccessibilityName() const { return accessibility_name_; }
    void AccessibilityName(hstring const& value) { accessibility_name_ = value; }
    bool HasDetails() const { return has_details_; }
    void HasDetails(bool value) { has_details_ = value; }
    bool Dismissible() const { return dismissible_; }
    void Dismissible(bool value) { dismissible_ = value; }
    bool IsMessage() const { return is_message_; }
    void IsMessage(bool value) { is_message_ = value; }
    bool IsProvisional() const { return is_provisional_; }
    void IsProvisional(bool value) { is_provisional_ = value; }

    Visibility MessageVisibility() const { return message_visibility_; }
    void MessageVisibility(Visibility value) { message_visibility_ = value; }
    Visibility CardVisibility() const { return card_visibility_; }
    void CardVisibility(Visibility value) { card_visibility_ = value; }
    Visibility ProgressVisibility() const { return progress_visibility_; }
    void ProgressVisibility(Visibility value) { progress_visibility_ = value; }
    Visibility SpineInfoVisibility() const { return spine_info_visibility_; }
    void SpineInfoVisibility(Visibility value) { spine_info_visibility_ = value; }
    Visibility SpineAttentionVisibility() const { return spine_attention_visibility_; }
    void SpineAttentionVisibility(Visibility value) { spine_attention_visibility_ = value; }
    Visibility SpineErrorVisibility() const { return spine_error_visibility_; }
    void SpineErrorVisibility(Visibility value) { spine_error_visibility_ = value; }
    Visibility SpineSuccessVisibility() const { return spine_success_visibility_; }
    void SpineSuccessVisibility(Visibility value) { spine_success_visibility_ = value; }
    Visibility ActionsVisibility() const { return actions_visibility_; }
    void ActionsVisibility(Visibility value) { actions_visibility_ = value; }
    Visibility DetailsVisibility() const { return details_visibility_; }
    void DetailsVisibility(Visibility value) { details_visibility_ = value; }
    Visibility StateVisibility() const { return state_visibility_; }
    void StateVisibility(Visibility value) { state_visibility_ = value; }
    Visibility ProvisionalVisibility() const { return provisional_visibility_; }
    void ProvisionalVisibility(Visibility value) { provisional_visibility_ = value; }
    Visibility TitleVisibility() const { return title_visibility_; }
    void TitleVisibility(Visibility value) { title_visibility_ = value; }
    Visibility BodyVisibility() const { return body_visibility_; }
    void BodyVisibility(Visibility value) { body_visibility_ = value; }
    Visibility DismissVisibility() const { return dismiss_visibility_; }
    void DismissVisibility(Visibility value) { dismiss_visibility_ = value; }
    Visibility TimestampVisibility() const { return timestamp_visibility_; }
    void TimestampVisibility(Visibility value) { timestamp_visibility_ = value; }

    winrt::Microsoft::UI::Xaml::Style CardStyle() const { return card_style_; }
    void CardStyle(winrt::Microsoft::UI::Xaml::Style const& value) { card_style_ = value; }

    winrt::Windows::Foundation::Collections::IObservableVector<winrt::Mayasaba::App::CardActionView>
    Actions() const {
        return actions_;
    }
    void Actions(
        winrt::Windows::Foundation::Collections::IObservableVector<winrt::Mayasaba::App::CardActionView> const& value) {
        actions_ = value;
    }

private:
    std::uint64_t sequence_{0};
    hstring card_id_{};
    hstring kind_{};
    hstring title_{};
    hstring body_{};
    hstring severity_{};
    hstring state_{};
    hstring timestamp_{};
    hstring author_{};
    hstring kind_label_{};
    hstring glyph_{};
    hstring accessibility_name_{};
    bool has_details_{false};
    bool dismissible_{false};
    bool is_message_{false};
    bool is_provisional_{false};
    Visibility message_visibility_{Visibility::Collapsed};
    Visibility card_visibility_{Visibility::Visible};
    Visibility progress_visibility_{Visibility::Collapsed};
    Visibility spine_info_visibility_{Visibility::Collapsed};
    Visibility spine_attention_visibility_{Visibility::Collapsed};
    Visibility spine_error_visibility_{Visibility::Collapsed};
    Visibility spine_success_visibility_{Visibility::Collapsed};
    Visibility actions_visibility_{Visibility::Collapsed};
    Visibility details_visibility_{Visibility::Collapsed};
    Visibility state_visibility_{Visibility::Collapsed};
    Visibility provisional_visibility_{Visibility::Collapsed};
    Visibility title_visibility_{Visibility::Visible};
    Visibility body_visibility_{Visibility::Visible};
    Visibility dismiss_visibility_{Visibility::Collapsed};
    Visibility timestamp_visibility_{Visibility::Collapsed};
    winrt::Microsoft::UI::Xaml::Style card_style_{nullptr};
    winrt::Windows::Foundation::Collections::IObservableVector<winrt::Mayasaba::App::CardActionView>
        actions_{nullptr};
};

// ---------------------------------------------------------------------------------------------
// AgentNotice
// ---------------------------------------------------------------------------------------------
struct AgentNotice : AgentNoticeT<AgentNotice> {
    AgentNotice() = default;

    hstring AgentId() const { return agent_id_; }
    void AgentId(hstring const& value) { agent_id_ = value; }
    hstring DisplayName() const { return display_name_; }
    void DisplayName(hstring const& value) { display_name_ = value; }
    hstring Readiness() const { return readiness_; }
    void Readiness(hstring const& value) { readiness_ = value; }
    hstring Reason() const { return reason_; }
    void Reason(hstring const& value) { reason_ = value; }
    hstring Effect() const { return effect_; }
    void Effect(hstring const& value) { effect_ = value; }
    hstring Guidance() const { return guidance_; }
    void Guidance(hstring const& value) { guidance_ = value; }
    hstring AccessibilityName() const { return accessibility_name_; }
    void AccessibilityName(hstring const& value) { accessibility_name_ = value; }
    Visibility LocateVisibility() const { return locate_visibility_; }
    void LocateVisibility(Visibility value) { locate_visibility_ = value; }
    Visibility ReasonVisibility() const { return reason_visibility_; }
    void ReasonVisibility(Visibility value) { reason_visibility_ = value; }

private:
    hstring agent_id_{};
    hstring display_name_{};
    hstring readiness_{};
    hstring reason_{};
    hstring effect_{};
    hstring guidance_{};
    hstring accessibility_name_{};
    Visibility locate_visibility_{Visibility::Collapsed};
    Visibility reason_visibility_{Visibility::Collapsed};
};

// ---------------------------------------------------------------------------------------------
// AttachmentChip
// ---------------------------------------------------------------------------------------------
struct AttachmentChip : AttachmentChipT<AttachmentChip> {
    AttachmentChip() = default;

    hstring Name() const { return name_; }
    void Name(hstring const& value) { name_ = value; }
    hstring SizeText() const { return size_text_; }
    void SizeText(hstring const& value) { size_text_ = value; }
    hstring StateText() const { return state_text_; }
    void StateText(hstring const& value) { state_text_ = value; }
    hstring AccessibilityName() const { return accessibility_name_; }
    void AccessibilityName(hstring const& value) { accessibility_name_ = value; }

private:
    hstring name_{};
    hstring size_text_{};
    hstring state_text_{};
    hstring accessibility_name_{};
};

}  // namespace winrt::Mayasaba::App::implementation

namespace winrt::Mayasaba::App::factory_implementation {

struct CardActionView : CardActionViewT<CardActionView, implementation::CardActionView> {};
struct TimelineCard : TimelineCardT<TimelineCard, implementation::TimelineCard> {};
struct AgentNotice : AgentNoticeT<AgentNotice, implementation::AgentNotice> {};
struct AttachmentChip : AttachmentChipT<AttachmentChip, implementation::AttachmentChip> {};

}  // namespace winrt::Mayasaba::App::factory_implementation

// The generated per-class sources define winrt_make_* and the projected constructors; a
// C++/WinRT project that declares its own runtimeclasses includes them from a compiled TU.
#include "CardActionView.g.cpp"
#include "TimelineCard.g.cpp"
#include "AgentNotice.g.cpp"
#include "AttachmentChip.g.cpp"

namespace Mayasaba::App {

namespace {

using namespace winrt::Microsoft::UI::Xaml;
using namespace winrt::Windows::Foundation::Collections;

Visibility Vis(bool visible) {
    return visible ? Visibility::Visible : Visibility::Collapsed;
}

winrt::hstring ToHString(const std::string& value) {
    return winrt::to_hstring(value);
}

// Resolves the per-kind card style from the application design system (Theme/Styles.xaml).
// A missing style degrades to the framework default rather than dropping the record.
Style LookupCardStyle(const std::string& kind) {
    try {
        auto resources = Application::Current().Resources();
        auto key = winrt::box_value(ToHString(mayasaba::app::StyleKeyForKind(kind)));
        if (resources.HasKey(key)) {
            return resources.Lookup(key).try_as<Style>();
        }
    } catch (...) {
    }
    return nullptr;
}

winrt::Mayasaba::App::CardActionView MakeCardActionView(const mayasaba::app::CardActionViewModel& vm) {
    winrt::Mayasaba::App::CardActionView action;
    action.ActionId(ToHString(vm.action_id));
    action.Label(ToHString(vm.label));
    action.Command(ToHString(vm.command));
    action.CardId(ToHString(vm.card_id));
    action.RequestJson(ToHString(vm.request_json));
    return action;
}

}  // namespace

winrt::Mayasaba::App::TimelineCard MakeTimelineCard(const mayasaba::app::TimelineItemViewModel& vm) {
    winrt::Mayasaba::App::TimelineCard card;
    card.Sequence(vm.sequence);
    card.CardId(ToHString(vm.card_id));
    card.Kind(ToHString(vm.kind));
    card.Title(ToHString(vm.title));
    card.Body(ToHString(vm.body));
    card.Severity(ToHString(vm.severity));
    card.State(ToHString(vm.state));
    card.Timestamp(ToHString(vm.timestamp));
    card.Author(ToHString(vm.author));
    card.KindLabel(ToHString(vm.kind_label));
    card.Glyph(ToHString(vm.glyph));
    card.AccessibilityName(ToHString(vm.accessibility_name));
    card.HasDetails(vm.has_details);
    card.Dismissible(vm.dismissible);
    card.IsMessage(vm.is_message);
    card.IsProvisional(vm.is_provisional);

    card.MessageVisibility(Vis(vm.is_message));
    card.CardVisibility(Vis(!vm.is_message));
    card.ProgressVisibility(Vis(vm.kind == mayasaba::app::kKindProgress));
    card.SpineInfoVisibility(Vis(vm.severity == "info"));
    card.SpineAttentionVisibility(Vis(vm.severity == "attention"));
    card.SpineErrorVisibility(Vis(vm.severity == "error"));
    card.SpineSuccessVisibility(Vis(vm.severity == "success"));
    card.ActionsVisibility(Vis(!vm.actions.empty() || vm.has_details || vm.dismissible));
    card.DetailsVisibility(Vis(vm.has_details));
    card.StateVisibility(Vis(!vm.state.empty()));
    card.ProvisionalVisibility(Vis(vm.is_provisional));
    card.TitleVisibility(Vis(!vm.title.empty()));
    card.BodyVisibility(Vis(!vm.body.empty() && vm.body != vm.title));
    card.DismissVisibility(Vis(vm.dismissible));
    card.TimestampVisibility(Vis(!vm.timestamp.empty()));

    card.CardStyle(LookupCardStyle(vm.kind));

    auto actions = winrt::single_threaded_observable_vector<winrt::Mayasaba::App::CardActionView>();
    for (const auto& action : vm.actions) {
        actions.Append(MakeCardActionView(action));
    }
    card.Actions(actions);
    return card;
}

winrt::Mayasaba::App::AgentNotice MakeAgentNotice(const mayasaba::app::AgentNoticeViewModel& vm) {
    winrt::Mayasaba::App::AgentNotice notice;
    notice.AgentId(ToHString(vm.agent_id));
    notice.DisplayName(ToHString(vm.display_name));
    notice.Readiness(ToHString(vm.readiness));
    notice.Reason(ToHString(vm.reason));
    notice.Effect(ToHString(vm.effect));
    notice.Guidance(ToHString(vm.guidance));
    notice.AccessibilityName(ToHString(vm.accessibility_name));
    notice.LocateVisibility(Vis(vm.can_locate));
    notice.ReasonVisibility(Vis(!vm.reason.empty()));
    return notice;
}

winrt::Mayasaba::App::AttachmentChip MakeAttachmentChip(const mayasaba::app::AttachmentViewModel& vm) {
    winrt::Mayasaba::App::AttachmentChip chip;
    chip.Name(ToHString(vm.name));
    chip.SizeText(ToHString(mayasaba::app::FormatByteSize(vm.size)));
    chip.StateText(ToHString(vm.state_text));
    chip.AccessibilityName(ToHString(vm.name + ", " + mayasaba::app::FormatByteSize(vm.size) +
                                     ", " + vm.state_text));
    return chip;
}

}  // namespace Mayasaba::App
