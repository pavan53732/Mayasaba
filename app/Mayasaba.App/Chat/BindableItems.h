// Mayasaba Control Room — factories from plain view-models to bindable WinRT items.
//
// The Chat page binds with x:Bind, so the objects a DataTemplate binds to must be
// WinRT-visible. These three factories are the only place that crosses from the plain C++
// view-model layer into WinRT; they copy strings and resolve the per-kind card Style, and do
// nothing else.
#pragma once

#include "pch.h"

// The bindable runtimeclasses named below (TimelineCard, AgentNotice, AttachmentChip,
// CardActionView) are declared in Project.idl; their projection must be visible to every
// translation unit that includes this header.
#include <winrt/Mayasaba.App.h>

#include "ChatViewModel.h"
#include "ComposerViewModel.h"
#include "TimelineItemViewModel.h"

namespace Mayasaba::App {

// Builds the bindable timeline card. Must be called on the UI thread (it resolves a Style from
// the application resource dictionary).
winrt::Mayasaba::App::TimelineCard MakeTimelineCard(const mayasaba::app::TimelineItemViewModel& vm);

winrt::Mayasaba::App::AgentNotice MakeAgentNotice(const mayasaba::app::AgentNoticeViewModel& vm);

winrt::Mayasaba::App::AttachmentChip MakeAttachmentChip(
    const mayasaba::app::AttachmentViewModel& vm);

}  // namespace Mayasaba::App
