// Mayasaba Control Room — Chat page code-behind.
//
// Thin by design: it wires XAML events to the plain C++ view-models, marshals controller
// notifications onto the UI thread, applies bounded batches of timeline updates, and manages
// focus and reading position. It contains no SQL, no process launch, no scheduling and no
// certification logic.
#pragma once

#include "pch.h"

#include "ChatPage.g.h"

#include "ChatViewModel.h"
#include "ComposerViewModel.h"
#include "ControllerSession.h"

#include <winrt/Windows.ApplicationModel.DataTransfer.h>

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace winrt::Mayasaba::App::implementation
{
    struct ChatPage : ChatPageT<ChatPage>
    {
        ChatPage();

        // Host window handle, needed only to parent the native folder/file pickers.
        void HostWindow(int64_t hwnd);

        // --- XAML-wired handlers (public so the generated Connect code can bind them) --------
        void OnLoaded(winrt::Windows::Foundation::IInspectable const& sender,
                      winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnUnloaded(winrt::Windows::Foundation::IInspectable const& sender,
                        winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnRootKeyDown(winrt::Windows::Foundation::IInspectable const& sender,
                           winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const& args);
        void OnComposerTextChanged(winrt::Windows::Foundation::IInspectable const& sender,
                                   winrt::Microsoft::UI::Xaml::Controls::TextChangedEventArgs const& args);
        void OnComposerKeyDown(winrt::Windows::Foundation::IInspectable const& sender,
                               winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const& args);
        void OnSendClick(winrt::Windows::Foundation::IInspectable const& sender,
                         winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnOpenFolderClick(winrt::Windows::Foundation::IInspectable const& sender,
                               winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnAttachClick(winrt::Windows::Foundation::IInspectable const& sender,
                           winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnRemoveAttachmentClick(winrt::Windows::Foundation::IInspectable const& sender,
                                     winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnStopClick(winrt::Windows::Foundation::IInspectable const& sender,
                         winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnCopyPathClick(winrt::Windows::Foundation::IInspectable const& sender,
                             winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnEnterSendsToggleClick(winrt::Windows::Foundation::IInspectable const& sender,
                                     winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnDiagnosticsClick(winrt::Windows::Foundation::IInspectable const& sender,
                                winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnCardActionClick(winrt::Windows::Foundation::IInspectable const& sender,
                               winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnShowDetailsClick(winrt::Windows::Foundation::IInspectable const& sender,
                                winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnDismissCardClick(winrt::Windows::Foundation::IInspectable const& sender,
                                winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnLocateExecutableClick(winrt::Windows::Foundation::IInspectable const& sender,
                                     winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnRecheckAgentClick(winrt::Windows::Foundation::IInspectable const& sender,
                                 winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnDetailsCloseClick(winrt::Windows::Foundation::IInspectable const& sender,
                                 winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnJumpToLatestClick(winrt::Windows::Foundation::IInspectable const& sender,
                                 winrt::Microsoft::UI::Xaml::RoutedEventArgs const& args);
        void OnComposerDragOver(winrt::Windows::Foundation::IInspectable const& sender,
                                winrt::Microsoft::UI::Xaml::DragEventArgs const& args);
        void OnComposerDrop(winrt::Windows::Foundation::IInspectable const& sender,
                            winrt::Microsoft::UI::Xaml::DragEventArgs const& args);

    private:
        // --- lifecycle ---------------------------------------------------------------------
        void StartController();
        void ApplyReducedMotionPreference();

        // --- refresh loop ------------------------------------------------------------------
        void RefreshNow();
        void AppendPendingCards();
        void UpdateHeader();
        void UpdateEmptyAndLoadingState();
        void UpdateAttentionNotices();
        void UpdateComposerUi();
        void UpdateStopButton();
        void UpdateJumpToLatest(bool appended);

        // --- commands ----------------------------------------------------------------------
        void SendCurrentDraft();
        void DismissCardById(std::wstring const& card_id);
        void InvokeCardAction(winrt::Mayasaba::App::CardActionView const& action);
        winrt::fire_and_forget OpenFolderPickerAsync();
        winrt::fire_and_forget AttachFilesPickerAsync();
        winrt::fire_and_forget LocateExecutableAsync(std::wstring agent_id);
        winrt::fire_and_forget HandleDropAsync(
            winrt::Windows::ApplicationModel::DataTransfer::DataPackageView const& view,
            winrt::Microsoft::UI::Xaml::DragOperationDeferral const& deferral);
        void ShowShellError(winrt::hstring const& title, winrt::hstring const& body);
        void ClearShellError();
        void AddAttachmentFromPath(std::wstring const& path, std::wstring const& name);

        // --- details sheet -----------------------------------------------------------------
        void OpenDetailsSheet(mayasaba::app::DetailsViewModel const& view_model,
                              winrt::Microsoft::UI::Xaml::FrameworkElement const& invoker);
        void CloseDetailsSheet();
        void ShowDiagnosticsSheet(winrt::Microsoft::UI::Xaml::FrameworkElement const& invoker);

        // --- helpers -----------------------------------------------------------------------
        bool TimelineIsAtBottom();
        void ScrollTimelineToEnd();

        mayasaba::app::ControllerSession session_;
        mayasaba::app::ChatViewModel chat_;
        mayasaba::app::ComposerViewModel composer_;

        winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer timer_{nullptr};
        winrt::Windows::Foundation::Collections::IObservableVector<winrt::Mayasaba::App::TimelineCard> cards_{nullptr};
        winrt::Windows::Foundation::Collections::IObservableVector<winrt::Mayasaba::App::AttachmentChip> chips_{nullptr};
        winrt::Windows::Foundation::Collections::IObservableVector<winrt::Mayasaba::App::AgentNotice> notices_{nullptr};

        winrt::weak_ref<ChatPage> self_{nullptr};
        winrt::weak_ref<winrt::Microsoft::UI::Xaml::FrameworkElement> details_invoker_{nullptr};

        int64_t hwnd_{0};
        bool loaded_{false};
        bool refreshing_{false};
        bool controller_ready_{false};
        bool reduced_motion_{false};
        bool details_open_{false};
        bool first_timeline_query_done_{false};
        bool user_at_bottom_{true};
        bool last_bound_state_{false};
        std::wstring active_operation_id_{};
        std::size_t selected_index_before_details_{0};
    };
}

namespace winrt::Mayasaba::App::factory_implementation
{
    struct ChatPage : ChatPageT<ChatPage, implementation::ChatPage>
    {
    };
}
