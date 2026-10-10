// Mayasaba Control Room — details sheet content builder.
//
// Builds the body of the temporary dismissible details sheet from an already-authoritative
// DetailsViewModel. This is presentation construction only (text, key/value rows, citations,
// and the bounded machine-readable record); it holds no state and calls no controller API, so
// the Chat page's code-behind stays thin.
#pragma once

#include "pch.h"

#include "ChatViewModel.h"

namespace Mayasaba::App {

void PopulateDetailsSheet(const winrt::Microsoft::UI::Xaml::Controls::Panel& host,
                          const mayasaba::app::DetailsViewModel& view_model);

}  // namespace Mayasaba::App
