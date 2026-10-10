// Mayasaba Control Room — the original Mayasaba mark.
#pragma once

#include "pch.h"

#include "LogoMark.g.h"

namespace winrt::Mayasaba::App::implementation
{
    struct LogoMark : LogoMarkT<LogoMark>
    {
        LogoMark();
    };
}

namespace winrt::Mayasaba::App::factory_implementation
{
    struct LogoMark : LogoMarkT<LogoMark, implementation::LogoMark>
    {
    };
}
