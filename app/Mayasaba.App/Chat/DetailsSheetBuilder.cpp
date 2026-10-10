// Mayasaba Control Room — details sheet content builder (see DetailsSheetBuilder.h).
#include "pch.h"

#include "DetailsSheetBuilder.h"

using winrt::Microsoft::UI::Xaml::Application;
using winrt::Microsoft::UI::Xaml::Style;
using winrt::Microsoft::UI::Xaml::TextWrapping;
using winrt::Microsoft::UI::Xaml::Controls::Border;
using winrt::Microsoft::UI::Xaml::Controls::ColumnDefinition;
using winrt::Microsoft::UI::Xaml::Controls::Expander;
using winrt::Microsoft::UI::Xaml::Controls::Grid;
using winrt::Microsoft::UI::Xaml::GridLength;
using winrt::Microsoft::UI::Xaml::GridUnitType;
using winrt::Microsoft::UI::Xaml::Controls::Panel;
using winrt::Microsoft::UI::Xaml::Controls::ScrollBarVisibility;
using winrt::Microsoft::UI::Xaml::Controls::ScrollViewer;
using winrt::Microsoft::UI::Xaml::Controls::StackPanel;
using winrt::Microsoft::UI::Xaml::Controls::TextBlock;

namespace Mayasaba::App {

namespace {

Style LookupStyle(const wchar_t* key) {
    try {
        auto resources = Application::Current().Resources();
        auto boxed = winrt::box_value(key);
        if (resources.HasKey(boxed)) {
            return resources.Lookup(boxed).try_as<Style>();
        }
    } catch (...) {
    }
    return nullptr;
}

TextBlock MakeText(const std::string& text, const wchar_t* style_key) {
    TextBlock block;
    block.Text(winrt::to_hstring(text));
    if (auto style = LookupStyle(style_key)) {
        block.Style(style);
    }
    block.IsTextSelectionEnabled(true);
    return block;
}

// One "Key    Value" row. The key is metadata (muted) and the value is selectable body text, so
// a screen reader reads a linear alternative to the visual two-column layout.
Grid MakeKeyValueRow(const std::string& key, const std::string& value) {
    Grid row;
    ColumnDefinition key_column;
    key_column.Width(GridLength{0.0, GridUnitType::Auto});
    ColumnDefinition value_column;
    value_column.Width(GridLength{1.0, GridUnitType::Star});
    row.ColumnDefinitions().Append(key_column);
    row.ColumnDefinitions().Append(value_column);
    row.ColumnSpacing(16);

    auto key_block = MakeText(key, L"TextMeta");
    key_block.MinWidth(150);
    Grid::SetColumn(key_block, 0);
    auto value_block = MakeText(value, L"TextBody");
    Grid::SetColumn(value_block, 1);
    row.Children().Append(key_block);
    row.Children().Append(value_block);
    return row;
}

}  // namespace

void PopulateDetailsSheet(const Panel& host, const mayasaba::app::DetailsViewModel& view_model) {
    host.Children().Clear();

    for (const auto& section : view_model.sections) {
        StackPanel block;
        block.Spacing(8);

        if (!section.title.empty()) {
            block.Children().Append(MakeText(section.title, L"TextSectionHeader"));
        }
        if (!section.text.empty()) {
            block.Children().Append(MakeText(section.text, L"TextBody"));
        }
        for (const auto& pair : section.key_values) {
            block.Children().Append(MakeKeyValueRow(pair.first, pair.second));
        }
        for (const auto& citation : section.citations) {
            Border citation_border;
            if (auto style = LookupStyle(L"CitationStyle")) {
                citation_border.Style(style);
            }
            StackPanel citation_stack;
            citation_stack.Spacing(4);
            if (!citation.source.empty()) {
                citation_stack.Children().Append(MakeText(citation.source, L"TextMono"));
            }
            if (!citation.span.empty()) {
                citation_stack.Children().Append(MakeText(citation.span, L"TextMono"));
            }
            if (!citation.text.empty()) {
                citation_stack.Children().Append(MakeText(citation.text, L"TextBody"));
            }
            citation_border.Child(citation_stack);
            block.Children().Append(citation_border);
        }

        host.Children().Append(block);
    }

    if (!view_model.raw_json.empty()) {
        // Dense machine-readable backing stays collapsed by default: structured inspection, not
        // conversational theatre. It is bounded by the view-model before it reaches here.
        Expander expander;
        expander.Header(winrt::box_value(winrt::hstring(L"Machine-readable record")));
        ScrollViewer scroller;
        scroller.MaxHeight(260.0);
        scroller.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        scroller.HorizontalScrollBarVisibility(ScrollBarVisibility::Auto);
        auto json_block = MakeText(view_model.raw_json, L"TextMono");
        json_block.TextWrapping(TextWrapping::NoWrap);
        scroller.Content(json_block);
        expander.Content(scroller);
        host.Children().Append(expander);
    }

    if (view_model.sections.empty() && view_model.raw_json.empty()) {
        // Explicit empty state: say what happened and what remains trustworthy.
        host.Children().Append(
            MakeText("No additional detail is recorded for this card yet.", L"TextBody"));
    }
}

}  // namespace Mayasaba::App
