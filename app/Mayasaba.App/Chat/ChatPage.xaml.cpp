// Mayasaba Control Room — Chat page code-behind implementation.
#include "pch.h"

#include "ChatPage.xaml.h"
#if __has_include("ChatPage.g.cpp")
#include "ChatPage.g.cpp"
#endif

#include "BindableItems.h"
#include "DetailsSheetBuilder.h"

#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include <winrt/Windows.Storage.h>
#include <winrt/Windows.Storage.Pickers.h>
#include <winrt/Windows.System.h>
#include <winrt/Windows.UI.ViewManagement.h>

#include <shobjidl_core.h>

#include <algorithm>
#include <chrono>
#include <cwctype>
#include <string>

using namespace winrt;
using namespace winrt::Microsoft::UI::Xaml;
using namespace winrt::Microsoft::UI::Xaml::Controls;
using namespace winrt::Microsoft::UI::Xaml::Input;
using namespace winrt::Microsoft::UI::Xaml::Media;
using namespace winrt::Microsoft::UI::Dispatching;
using namespace winrt::Windows::Foundation;
using namespace winrt::Windows::Foundation::Collections;
using namespace winrt::Windows::Storage;
using namespace winrt::Windows::Storage::Pickers;
using namespace winrt::Windows::ApplicationModel::DataTransfer;

// Presentation helpers declared in the app's own namespace (Chat/BindableItems.h,
// Chat/DetailsSheetBuilder.h). The leading :: is required: from
// winrt::Mayasaba::App::implementation a bare `Mayasaba::App` would resolve to
// winrt::Mayasaba::App, so the app namespace must be named from the global scope.
using namespace ::Mayasaba::App;

namespace
{
    constexpr auto kPollInterval = std::chrono::milliseconds(700);
    constexpr std::uint32_t kMaxUiCards = 500;
    constexpr double kAtBottomSlack = 24.0;

    template <typename T>
    T FindDescendant(DependencyObject const& root)
    {
        if (!root)
        {
            return nullptr;
        }
        const int count = VisualTreeHelper::GetChildrenCount(root);
        for (int i = 0; i < count; ++i)
        {
            auto child = VisualTreeHelper::GetChild(root, i);
            if (auto typed = child.try_as<T>())
            {
                return typed;
            }
            if (auto found = FindDescendant<T>(child))
            {
                return found;
            }
        }
        return nullptr;
    }

    bool CaseInsensitivePrefixMatch(std::wstring const& path, std::wstring const& prefix)
    {
        if (prefix.empty() || path.size() < prefix.size())
        {
            return false;
        }
        for (std::size_t i = 0; i < prefix.size(); ++i)
        {
            if (std::towlower(path[i]) != std::towlower(prefix[i]))
            {
                return false;
            }
        }
        return true;
    }

    bool IsTerminalState(std::string const& state)
    {
        return state == "completed" || state == "complete" || state == "done" || state == "cancelled" ||
               state == "canceled" || state == "failed" || state == "stopped" || state == "idle";
    }
}

#pragma warning(push, 0)
namespace
{
    // Reads one string field out of a card action's typed request payload. Returns empty when
    // the payload is absent, malformed or carries a different shape, so nothing is invented.
    std::string JsonStringField(std::string const& json_text, char const* field)
    {
        if (json_text.empty())
        {
            return {};
        }
        try
        {
            auto parsed = nlohmann::json::parse(json_text);
            if (parsed.is_object())
            {
                auto it = parsed.find(field);
                if (it != parsed.end() && it->is_string())
                {
                    return it->get<std::string>();
                }
            }
        }
        catch (...)
        {
        }
        return {};
    }
}
#pragma warning(pop)

namespace winrt::Mayasaba::App::implementation
{
    ChatPage::ChatPage()
    {
        InitializeComponent();
        self_ = get_weak();
    }

    void ChatPage::HostWindow(int64_t hwnd)
    {
        hwnd_ = hwnd;
    }

    // =========================================================================================
    // Lifecycle
    // =========================================================================================
    void ChatPage::OnLoaded(IInspectable const&, RoutedEventArgs const&)
    {
        if (loaded_)
        {
            return;
        }
        loaded_ = true;

        cards_ = single_threaded_observable_vector<winrt::Mayasaba::App::TimelineCard>();
        chips_ = single_threaded_observable_vector<winrt::Mayasaba::App::AttachmentChip>();
        notices_ = single_threaded_observable_vector<winrt::Mayasaba::App::AgentNotice>();
        TimelineList().ItemsSource(cards_);
        AttachmentChips().ItemsSource(chips_);
        AgentAttentionStrip().ItemsSource(notices_);

        ApplyReducedMotionPreference();

        UpdateHeader();
        UpdateComposerUi();
        UpdateEmptyAndLoadingState();
        UpdateStopButton();

        LoadingState().Visibility(Visibility::Visible);

        StartController();

        timer_ = DispatcherQueue().CreateTimer();
        timer_.Interval(kPollInterval);
        auto weak = self_;
        timer_.Tick([weak](DispatcherQueueTimer const&, IInspectable const&) {
            if (auto page = weak.get())
            {
                page->RefreshNow();
            }
        });
        timer_.Start();

        RefreshNow();
    }

    void ChatPage::OnUnloaded(IInspectable const&, RoutedEventArgs const&)
    {
        if (!loaded_)
        {
            return;
        }
        loaded_ = false;
        if (timer_)
        {
            timer_.Stop();
            timer_ = nullptr;
        }
        session_.Stop();
        controller_ready_ = false;
    }

    void ChatPage::StartController()
    {
        auto weak = self_;
        // The controller calls this from one of its own threads. Marshal onto the UI thread and
        // re-query there; never touch XAML from the controller thread.
        auto status = session_.Start([weak]() {
            if (auto page = weak.get())
            {
                page->DispatcherQueue().TryEnqueue([weak]() {
                    if (auto page2 = weak.get())
                    {
                        page2->RefreshNow();
                    }
                });
            }
        });

        if (!status.ok())
        {
            controller_ready_ = false;
            chat_.Attach(nullptr);
            LoadingState().Visibility(Visibility::Collapsed);
            first_timeline_query_done_ = true;
            ShowShellError(L"Mayasaba could not start",
                           to_hstring(std::string("The controller reported: ") + status.ToString()));
            UpdateEmptyAndLoadingState();
            return;
        }

        controller_ready_ = true;
        chat_.Attach(session_.Controller());
    }

    void ChatPage::ApplyReducedMotionPreference()
    {
        bool animations_enabled = true;
        try
        {
            winrt::Windows::UI::ViewManagement::UISettings settings;
            animations_enabled = settings.AnimationsEnabled();
        }
        catch (...)
        {
        }
        reduced_motion_ = !animations_enabled;
        if (reduced_motion_)
        {
            // Reduced motion: insertion/collapse snap straight to their final state.
            TimelineList().ItemContainerTransitions().Clear();
        }
    }

    // =========================================================================================
    // Refresh loop — bounded batches, no forced scrolling while the user is reading
    // =========================================================================================
    void ChatPage::RefreshNow()
    {
        if (!loaded_ || refreshing_ || !controller_ready_)
        {
            return;
        }
        refreshing_ = true;
        struct Guard
        {
            bool& flag;
            ~Guard() { flag = false; }
        } guard{refreshing_};

        hstring error;
        auto project = chat_.RefreshProjectState();
        if (!project.ok())
        {
            error = to_hstring(project.ToString());
        }
        auto agents = chat_.RefreshAgents();
        if (!agents.ok() && error.empty())
        {
            error = to_hstring(agents.ToString());
        }
        auto timeline = chat_.RefreshTimeline();
        if (!timeline.ok() && error.empty())
        {
            error = to_hstring(timeline.ToString());
        }

        if (!error.empty())
        {
            // Preserve readable state and say exactly what failed.
            ShowShellError(L"Mayasaba could not read current state", error);
        }
        else
        {
            ClearShellError();
        }

        first_timeline_query_done_ = true;

        AppendPendingCards();
        UpdateHeader();
        UpdateAttentionNotices();
        UpdateEmptyAndLoadingState();
        UpdateStopButton();

        const bool bound = chat_.ProjectState().bound;
        if (bound != last_bound_state_)
        {
            last_bound_state_ = bound;
            UpdateComposerUi();
        }
    }

    void ChatPage::AppendPendingCards()
    {
        auto pending = chat_.TakePendingItems();
        if (pending.empty())
        {
            UpdateJumpToLatest(false);
            return;
        }

        const bool was_at_bottom = TimelineIsAtBottom();
        for (auto const& item : pending)
        {
            cards_.Append(MakeTimelineCard(item));
            if (item.kind == mayasaba::app::kKindProgress)
            {
                // The card's own id is the operation id the Stop control cancels.
                active_operation_id_ =
                    (item.state.empty() || !IsTerminalState(item.state))
                        ? std::wstring(to_hstring(item.card_id).c_str())
                        : std::wstring{};
            }
        }

        // Bounded in-memory list: the UI keeps a window over history, not an unbounded log.
        while (cards_.Size() > kMaxUiCards)
        {
            cards_.RemoveAt(0);
        }

        if (was_at_bottom)
        {
            ScrollTimelineToEnd();
        }
        UpdateJumpToLatest(true);
    }

    void ChatPage::UpdateHeader()
    {
        const auto& project = chat_.ProjectState();
        if (project.bound)
        {
            std::string display = project.display_path;
            auto separator = display.find_last_of("\\/");
            std::string leaf = (separator == std::string::npos) ? display : display.substr(separator + 1);
            if (leaf.empty())
            {
                leaf = display;
            }
            ProjectNameText().Text(leaf.empty() ? hstring(L"Project") : to_hstring(leaf));
            ProjectPathText().Text(to_hstring(project.canonical_root));
            ProjectPathText().Visibility(Visibility::Visible);
            OpenFolderButtonText().Text(leaf.empty() ? hstring(L"Switch Folder") : to_hstring(leaf));
        }
        else
        {
            ProjectNameText().Text(L"No project bound");
            ProjectPathText().Text(L"");
            ProjectPathText().Visibility(Visibility::Collapsed);
            OpenFolderButtonText().Text(L"Open Folder");
        }

        PhaseText().Text(project.bound && !project.phase.empty()
                             ? to_hstring(std::string("Phase: ") + project.phase)
                             : hstring{});
        ConditionText().Text(project.bound
                                 ? to_hstring(std::string("Condition: ") + chat_.DerivedOperationalCondition())
                                 : hstring{});
    }

    void ChatPage::UpdateAttentionNotices()
    {
        notices_.Clear();
        for (auto const& notice : chat_.AgentNotices())
        {
            notices_.Append(MakeAgentNotice(notice));
        }
        AgentAttentionStrip().Visibility(notices_.Size() > 0 ? Visibility::Visible : Visibility::Collapsed);

        bool checking = false;
        for (auto const& agent : chat_.AllAgents())
        {
            if (agent.readiness == "CHECKING")
            {
                checking = true;
            }
        }
        CheckingRing().IsActive(checking);
        CheckingRing().Visibility(checking ? Visibility::Visible : Visibility::Collapsed);
    }

    void ChatPage::UpdateEmptyAndLoadingState()
    {
        const bool has_cards = cards_.Size() > 0;
        const bool bound = chat_.ProjectState().bound;
        const bool show_loading = !has_cards && !first_timeline_query_done_;
        const bool show_empty = !has_cards && !show_loading;

        LoadingState().Visibility(show_loading ? Visibility::Visible : Visibility::Collapsed);
        EmptyState().Visibility(show_empty ? Visibility::Visible : Visibility::Collapsed);
        TimelineList().Visibility(has_cards ? Visibility::Visible : Visibility::Collapsed);

        if (show_empty)
        {
            if (bound)
            {
                EmptyTitle().Text(L"No messages yet");
                EmptyBody().Text(
                    L"The project root is bound. Send a message to record the first contribution — every Send "
                    L"persists exactly one immutable contribution, even when no CLI is available.");
                EmptyHint().Text(L"Send is enabled.");
            }
            else
            {
                EmptyTitle().Text(L"One project reality, three agent sessions");
                EmptyBody().Text(
                    L"Mayasaba keeps one authoritative record of your project while Hermes, Kilo Code and OpenCode "
                    L"work as separate sessions. Open a folder to bind the project root — opening it does not scan, "
                    L"launch or change anything.");
                EmptyHint().Text(L"Send stays disabled until an authorized project root is bound.");
            }
        }
    }

    void ChatPage::UpdateComposerUi()
    {
        const bool bound = chat_.ProjectState().bound;
        composer_.SetBound(bound);

        if (ComposerBox().Text() != to_hstring(composer_.Draft()))
        {
            ComposerBox().Text(to_hstring(composer_.Draft()));
        }
        SendButton().IsEnabled(composer_.CanSend());
        SendHintText().Text(bound ? hstring{} : hstring(L"Open a folder to send."));
        SendHintText().Visibility(bound ? Visibility::Collapsed : Visibility::Visible);

        chips_.Clear();
        for (auto const& attachment : composer_.Attachments())
        {
            chips_.Append(MakeAttachmentChip(attachment));
        }
        AttachmentChips().Visibility(chips_.Size() > 0 ? Visibility::Visible : Visibility::Collapsed);
    }

    void ChatPage::UpdateStopButton()
    {
        StopButton().IsEnabled(!active_operation_id_.empty());
    }

    void ChatPage::UpdateJumpToLatest(bool appended)
    {
        if (!appended || cards_.Size() == 0)
        {
            JumpToLatestButton().Visibility(Visibility::Collapsed);
            return;
        }
        // Never force-scroll while the user is reading: offer an explicit action instead.
        JumpToLatestButton().Visibility(TimelineIsAtBottom() ? Visibility::Collapsed : Visibility::Visible);
    }

    // =========================================================================================
    // Commands
    // =========================================================================================
    void ChatPage::SendCurrentDraft()
    {
        if (!composer_.CanSend())
        {
            return;
        }
        const std::string text = composer_.Draft();
        const auto attachments = composer_.ToAttachmentRefs();

        auto receipt = chat_.Send(text, attachments);
        if (!receipt.ok())
        {
            // Rejected submission preserves the draft and every valid attachment.
            ShowShellError(L"Message was not persisted", to_hstring(receipt.status().ToString()));
            return;
        }

        const auto& value = receipt.value();
        if (value.outcome == "PERSISTED")
        {
            ClearShellError();
            composer_.ClearAfterPersisted();
            UpdateComposerUi();
            RefreshNow();
        }
        else
        {
            std::string detail = value.outcome;
            if (!value.detail.empty())
            {
                detail += ": " + value.detail;
            }
            ShowShellError(L"Message was not persisted", to_hstring(detail));
        }
    }

    void ChatPage::DismissCardById(std::wstring const& card_id)
    {
        auto ack = chat_.DismissCard(to_string(hstring(card_id)));
        if (!ack.ok())
        {
            ShowShellError(L"Card could not be dismissed", to_hstring(ack.status().ToString()));
            return;
        }
        // Presentation filtering only: the authoritative record is untouched.
        const hstring wanted = hstring(card_id);
        for (std::uint32_t i = 0; i < cards_.Size(); ++i)
        {
            if (cards_.GetAt(i).CardId() == wanted)
            {
                cards_.RemoveAt(i);
                break;
            }
        }
        UpdateEmptyAndLoadingState();
        UpdateJumpToLatest(false);
    }

    void ChatPage::InvokeCardAction(winrt::Mayasaba::App::CardActionView const& action)
    {
        const std::string command = to_string(action.Command());
        const std::string card_id = to_string(action.CardId());
        const std::string request = to_string(action.RequestJson());

        if (command == "RetryAgentProbe")
        {
            const std::string agent_id = JsonStringField(request, "agent_id");
            if (agent_id.empty())
            {
                ShowShellError(L"Recheck needs an agent", L"The card action did not name a CLI.");
                return;
            }
            auto probe = chat_.RetryAgentProbe(agent_id);
            if (!probe.ok())
            {
                ShowShellError(L"Recheck failed", to_hstring(probe.status().ToString()));
                return;
            }
            RefreshNow();
            return;
        }
        if (command == "CancelOperation")
        {
            auto ack = chat_.CancelOperation(card_id);
            if (!ack.ok())
            {
                ShowShellError(L"Stop failed", to_hstring(ack.status().ToString()));
                return;
            }
            RefreshNow();
            return;
        }
        if (command == "DismissCard")
        {
            DismissCardById(std::wstring(to_hstring(card_id).c_str()));
            return;
        }
        if (command == "SetAgentExecutablePath")
        {
            const std::string agent_id = JsonStringField(request, "agent_id");
            if (!agent_id.empty())
            {
                LocateExecutableAsync(std::wstring(to_hstring(agent_id).c_str()));
            }
            return;
        }
        if (command == "RequestDetails")
        {
            auto details = chat_.RequestDetailsView(card_id);
            if (!details.ok())
            {
                ShowShellError(L"Details are unavailable", to_hstring(details.status().ToString()));
                return;
            }
            OpenDetailsSheet(details.value(), OverflowButton());
            return;
        }

        // Fail closed and say why: this build only knows the registered card actions.
        ShowShellError(L"Action not supported by this build",
                       to_hstring(std::string("The card asked for '") + command +
                                  "', which this Control Room build does not invoke."));
    }

    void ChatPage::ShowShellError(hstring const& title, hstring const& body)
    {
        ShellErrorTitle().Text(title);
        ShellErrorBody().Text(body);
        ShellErrorCard().Visibility(Visibility::Visible);
    }

    void ChatPage::ClearShellError()
    {
        ShellErrorCard().Visibility(Visibility::Collapsed);
    }

    void ChatPage::AddAttachmentFromPath(std::wstring const& path, std::wstring const& name)
    {
        if (path.empty())
        {
            return;
        }
        mayasaba::app::AttachmentViewModel attachment;
        attachment.name = to_string(hstring(name.empty() ? path : name));
        attachment.path = to_string(hstring(path));

        WIN32_FILE_ATTRIBUTE_DATA attributes{};
        if (::GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &attributes) != 0)
        {
            attachment.size = (static_cast<std::uint64_t>(attributes.nFileSizeHigh) << 32) |
                              static_cast<std::uint64_t>(attributes.nFileSizeLow);
        }

        // Honest boundary labelling: a file outside the canonical root is authorized
        // individually by the controller, never by proximity to the project.
        const auto& project = chat_.ProjectState();
        const bool outside =
            project.bound && !project.canonical_root.empty() &&
            !CaseInsensitivePrefixMatch(path, std::wstring(to_hstring(project.canonical_root).c_str()));
        attachment.state_text = outside ? "Selected · outside project root" : "Selected";

        composer_.AddAttachment(std::move(attachment));
        UpdateComposerUi();
    }

    // =========================================================================================
    // Pickers and drag/drop
    // =========================================================================================
    fire_and_forget ChatPage::OpenFolderPickerAsync()
    {
        auto lifetime = get_strong();
        FolderPicker picker;
        picker.FileTypeFilter().Append(L"*");
        picker.SuggestedStartLocation(PickerLocationId::ComputerFolder);
        if (hwnd_ != 0)
        {
            picker.as<::IInitializeWithWindow>()->Initialize(reinterpret_cast<HWND>(hwnd_));
        }

        auto folder = co_await picker.PickSingleFolderAsync();
        if (!folder)
        {
            co_return;
        }

        const std::string display_path = to_string(folder.Path());
        auto result = chat_.OpenFolder(display_path);
        if (!result.ok())
        {
            ShowShellError(L"Folder was not bound", to_hstring(result.status().ToString()));
            co_return;
        }
        const auto& value = result.value();
        if (!value.ok)
        {
            // Exact rejection reason; the draft is preserved and Open Folder stays available.
            ShowShellError(L"Folder was not bound",
                           value.detail.empty() ? hstring(L"The controller rejected this folder.")
                                                : to_hstring(value.detail));
            co_return;
        }
        ClearShellError();
        RefreshNow();
    }

    fire_and_forget ChatPage::AttachFilesPickerAsync()
    {
        auto lifetime = get_strong();
        FileOpenPicker picker;
        picker.FileTypeFilter().Append(L"*");
        picker.SuggestedStartLocation(PickerLocationId::DocumentsLibrary);
        if (hwnd_ != 0)
        {
            picker.as<::IInitializeWithWindow>()->Initialize(reinterpret_cast<HWND>(hwnd_));
        }

        auto files = co_await picker.PickMultipleFilesAsync();
        if (!files)
        {
            co_return;
        }
        for (auto const& file : files)
        {
            // Selecting files never submits the draft.
            AddAttachmentFromPath(std::wstring(file.Path().c_str()), std::wstring(file.Name().c_str()));
        }
    }

    fire_and_forget ChatPage::LocateExecutableAsync(std::wstring agent_id)
    {
        auto lifetime = get_strong();
        FileOpenPicker picker;
        picker.FileTypeFilter().Append(L".exe");
        picker.SuggestedStartLocation(PickerLocationId::ComputerFolder);
        if (hwnd_ != 0)
        {
            picker.as<::IInitializeWithWindow>()->Initialize(reinterpret_cast<HWND>(hwnd_));
        }

        auto file = co_await picker.PickSingleFileAsync();
        if (!file)
        {
            co_return;
        }

        // The path is the user's own choice. Mayasaba never guesses an executable location and
        // never writes model/provider configuration.
        auto ack = chat_.SetAgentExecutablePath(to_string(hstring(agent_id)), to_string(file.Path()));
        if (!ack.ok())
        {
            ShowShellError(L"Executable path was not accepted", to_hstring(ack.status().ToString()));
            co_return;
        }
        RefreshNow();
    }

    fire_and_forget ChatPage::HandleDropAsync(DataPackageView const& view,
                                             DragOperationDeferral const& deferral)
    {
        auto lifetime = get_strong();
        try
        {
            auto items = co_await view.GetStorageItemsAsync();
            for (auto const& item : items)
            {
                if (auto file = item.try_as<StorageFile>())
                {
                    AddAttachmentFromPath(std::wstring(file.Path().c_str()),
                                          std::wstring(file.Name().c_str()));
                }
            }
        }
        catch (...)
        {
            // A failed drop never discards valid selections and never submits the draft.
        }
        deferral.Complete();
    }

    // =========================================================================================
    // Details sheet — inline/sheet disclosure, never a route
    // =========================================================================================
    void ChatPage::OpenDetailsSheet(mayasaba::app::DetailsViewModel const& view_model,
                                    FrameworkElement const& invoker)
    {
        if (invoker)
        {
            details_invoker_ = winrt::make_weak(invoker);
        }
        else
        {
            details_invoker_ = nullptr;
        }
        selected_index_before_details_ = TimelineList().SelectedIndex() >= 0
                                             ? static_cast<std::size_t>(TimelineList().SelectedIndex())
                                             : 0;

        PopulateDetailsSheet(DetailsSections(), view_model);
        DetailsTitle().Text(to_hstring(view_model.title));
        DetailsCardIdText().Text(to_hstring(view_model.card_id));
        DetailsSubtitle().Text(L"Temporary sheet over Chat · close with Close or Escape");

        DetailsOverlay().Visibility(Visibility::Visible);
        details_open_ = true;
        DetailsCloseButton().Focus(FocusState::Programmatic);

        if (!reduced_motion_)
        {
            try
            {
                using namespace winrt::Microsoft::UI::Xaml::Media::Animation;
                DoubleAnimation fade;
                fade.From(0.0);
                fade.To(1.0);
                fade.Duration(Duration{TimeSpan{std::chrono::milliseconds(200)}, DurationType::TimeSpan});
                Storyboard storyboard;
                storyboard.Children().Append(fade);
                Storyboard::SetTarget(fade, DetailsSheet());
                Storyboard::SetTargetProperty(fade, L"Opacity");
                storyboard.Begin();
            }
            catch (...)
            {
                // If the reveal cannot run, the sheet is already visible.
            }
        }
    }

    void ChatPage::CloseDetailsSheet()
    {
        DetailsOverlay().Visibility(Visibility::Collapsed);
        details_open_ = false;
        DetailsSections().Children().Clear();

        if (auto invoker = details_invoker_.get())
        {
            invoker.Focus(FocusState::Programmatic);
        }
        details_invoker_ = nullptr;

        // Restore the selected card so keyboard and screen-reader context return to Chat.
        if (selected_index_before_details_ < cards_.Size())
        {
            TimelineList().ScrollIntoView(cards_.GetAt(static_cast<std::uint32_t>(selected_index_before_details_)));
        }
    }

    void ChatPage::ShowDiagnosticsSheet(FrameworkElement const& invoker)
    {
        mayasaba::app::DetailsViewModel view_model;
        view_model.title = "Technical diagnostics";
        view_model.card_id = "diagnostics";
        view_model.accessibility_name = "Technical diagnostics";

        mayasaba::app::DetailsSectionViewModel intro;
        intro.title = "Observed adapter readiness";
        intro.text =
            "Read-only. This sheet exposes observed readiness, the exact observed reason and executable "
            "identity only. It never shows credentials, authentication tokens, private prompts, model "
            "configuration or unrelated process output.";
        view_model.sections.push_back(std::move(intro));

        for (auto const& agent : chat_.AllAgents())
        {
            mayasaba::app::DetailsSectionViewModel section;
            section.title = agent.display_name.empty() ? agent.agent_id : agent.display_name;
            section.key_values.push_back({"Agent id", agent.agent_id});
            section.key_values.push_back({"Readiness", agent.readiness});
            section.key_values.push_back({"Needs attention", agent.needs_attention ? "yes" : "no"});
            if (!agent.reason.empty())
            {
                section.key_values.push_back({"Observed reason", agent.reason});
            }
            section.key_values.push_back(
                {"Executable", agent.executable_path.empty() ? "(not configured)" : agent.executable_path});
            view_model.sections.push_back(std::move(section));
        }

        mayasaba::app::DetailsSectionViewModel limits;
        limits.title = "Limitations of this contract version";
        limits.text =
            "The controller's versioned agent query exposes observed readiness, reason and executable "
            "identity. Probe timestamps, capability lists and recovery history are not yet part of the "
            "query contract, so this sheet does not claim to show them.";
        view_model.sections.push_back(std::move(limits));

        OpenDetailsSheet(view_model, invoker);
    }

    // =========================================================================================
    // Helpers
    // =========================================================================================
    bool ChatPage::TimelineIsAtBottom()
    {
        auto scroller = FindDescendant<ScrollViewer>(TimelineList());
        if (!scroller)
        {
            return true;
        }
        return (scroller.ScrollableHeight() - scroller.VerticalOffset()) < kAtBottomSlack;
    }

    void ChatPage::ScrollTimelineToEnd()
    {
        if (cards_.Size() == 0)
        {
            return;
        }
        TimelineList().ScrollIntoView(cards_.GetAt(cards_.Size() - 1));
    }

    // =========================================================================================
    // XAML event handlers
    // =========================================================================================
    void ChatPage::OnRootKeyDown(IInspectable const&, KeyRoutedEventArgs const& args)
    {
        if (args.Key() == winrt::Windows::System::VirtualKey::Escape && details_open_)
        {
            args.Handled(true);
            CloseDetailsSheet();
        }
    }

    void ChatPage::OnComposerTextChanged(IInspectable const&, TextChangedEventArgs const&)
    {
        composer_.SetDraft(to_string(ComposerBox().Text()));
        SendButton().IsEnabled(composer_.CanSend());
    }

    void ChatPage::OnComposerKeyDown(IInspectable const&, KeyRoutedEventArgs const& args)
    {
        if (args.Key() != winrt::Windows::System::VirtualKey::Enter)
        {
            return;
        }
        if (!composer_.EnterSends())
        {
            return;
        }
        const bool shift = (::GetKeyState(VK_SHIFT) & 0x8000) != 0;
        if (shift)
        {
            // Shift+Enter keeps the multiline entry behaviour available to everyone.
            return;
        }
        args.Handled(true);
        SendCurrentDraft();
    }

    void ChatPage::OnSendClick(IInspectable const&, RoutedEventArgs const&)
    {
        SendCurrentDraft();
    }

    void ChatPage::OnOpenFolderClick(IInspectable const&, RoutedEventArgs const&)
    {
        OpenFolderPickerAsync();
    }

    void ChatPage::OnAttachClick(IInspectable const&, RoutedEventArgs const&)
    {
        AttachFilesPickerAsync();
    }

    void ChatPage::OnRemoveAttachmentClick(IInspectable const& sender, RoutedEventArgs const&)
    {
        auto element = sender.try_as<FrameworkElement>();
        if (!element)
        {
            return;
        }
        auto chip = element.DataContext().try_as<winrt::Mayasaba::App::AttachmentChip>();
        if (!chip)
        {
            return;
        }
        const std::string name = to_string(chip.Name());
        const auto& attachments = composer_.Attachments();
        for (std::size_t i = 0; i < attachments.size(); ++i)
        {
            if (attachments[i].name == name)
            {
                composer_.RemoveAttachmentAt(i);
                break;
            }
        }
        UpdateComposerUi();
    }

    void ChatPage::OnStopClick(IInspectable const&, RoutedEventArgs const&)
    {
        if (active_operation_id_.empty())
        {
            return;
        }
        auto ack = chat_.CancelOperation(to_string(hstring(active_operation_id_)));
        if (!ack.ok())
        {
            ShowShellError(L"Stop failed", to_hstring(ack.status().ToString()));
            return;
        }
        RefreshNow();
    }

    void ChatPage::OnCopyPathClick(IInspectable const&, RoutedEventArgs const&)
    {
        const auto& project = chat_.ProjectState();
        if (project.canonical_root.empty())
        {
            return;
        }
        try
        {
            DataPackage package;
            package.SetText(to_hstring(project.canonical_root));
            Clipboard::SetContent(package);
            Clipboard::Flush();
        }
        catch (...)
        {
            ShowShellError(L"Path could not be copied", L"The clipboard was unavailable.");
        }
    }

    void ChatPage::OnEnterSendsToggleClick(IInspectable const&, RoutedEventArgs const&)
    {
        composer_.SetEnterSends(MenuEnterSends().IsChecked());
    }

    void ChatPage::OnDiagnosticsClick(IInspectable const&, RoutedEventArgs const&)
    {
        ShowDiagnosticsSheet(OverflowButton());
    }

    void ChatPage::OnCardActionClick(IInspectable const& sender, RoutedEventArgs const&)
    {
        auto element = sender.try_as<FrameworkElement>();
        if (!element)
        {
            return;
        }
        // The button's DataContext is the CardActionView it was generated for.
        auto action = element.DataContext().try_as<winrt::Mayasaba::App::CardActionView>();
        if (!action)
        {
            return;
        }
        InvokeCardAction(action);
    }

    void ChatPage::OnShowDetailsClick(IInspectable const& sender, RoutedEventArgs const&)
    {
        auto element = sender.try_as<FrameworkElement>();
        if (!element)
        {
            return;
        }
        auto card = element.DataContext().try_as<winrt::Mayasaba::App::TimelineCard>();
        if (!card)
        {
            return;
        }
        auto details = chat_.RequestDetailsView(to_string(card.CardId()));
        if (!details.ok())
        {
            ShowShellError(L"Details are unavailable", to_hstring(details.status().ToString()));
            return;
        }
        OpenDetailsSheet(details.value(), element);
    }

    void ChatPage::OnDismissCardClick(IInspectable const& sender, RoutedEventArgs const&)
    {
        auto element = sender.try_as<FrameworkElement>();
        if (!element)
        {
            return;
        }
        auto card = element.DataContext().try_as<winrt::Mayasaba::App::TimelineCard>();
        if (!card)
        {
            return;
        }
        DismissCardById(std::wstring(card.CardId().c_str()));
    }

    void ChatPage::OnLocateExecutableClick(IInspectable const& sender, RoutedEventArgs const&)
    {
        auto element = sender.try_as<FrameworkElement>();
        if (!element)
        {
            return;
        }
        auto notice = element.DataContext().try_as<winrt::Mayasaba::App::AgentNotice>();
        if (!notice)
        {
            return;
        }
        LocateExecutableAsync(std::wstring(notice.AgentId().c_str()));
    }

    void ChatPage::OnRecheckAgentClick(IInspectable const& sender, RoutedEventArgs const&)
    {
        auto element = sender.try_as<FrameworkElement>();
        if (!element)
        {
            return;
        }
        auto notice = element.DataContext().try_as<winrt::Mayasaba::App::AgentNotice>();
        if (!notice)
        {
            return;
        }
        // Recheck observes status only: it never installs, updates or reconfigures the CLI.
        auto probe = chat_.RetryAgentProbe(to_string(notice.AgentId()));
        if (!probe.ok())
        {
            ShowShellError(L"Recheck failed", to_hstring(probe.status().ToString()));
            return;
        }
        RefreshNow();
    }

    void ChatPage::OnDetailsCloseClick(IInspectable const&, RoutedEventArgs const&)
    {
        CloseDetailsSheet();
    }

    void ChatPage::OnJumpToLatestClick(IInspectable const&, RoutedEventArgs const&)
    {
        ScrollTimelineToEnd();
        JumpToLatestButton().Visibility(Visibility::Collapsed);
    }

    void ChatPage::OnComposerDragOver(IInspectable const&, DragEventArgs const& args)
    {
        args.AcceptedOperation(DataPackageOperation::Copy);
        args.DragUIOverride().IsCaptionVisible(false);
    }

    void ChatPage::OnComposerDrop(IInspectable const&, DragEventArgs const& args)
    {
        args.AcceptedOperation(DataPackageOperation::Copy);
        auto deferral = args.GetDeferral();
        HandleDropAsync(args.DataView(), deferral);
    }
}
