// Mayasaba Control Room shell — Application subclass declaration.
#pragma once

#include "pch.h"
#include "App.xaml.g.h"

namespace winrt::Mayasaba::App::implementation
{
    struct App : AppT<App>
    {
        App();

        void OnLaunched(Microsoft::UI::Xaml::LaunchActivatedEventArgs const&);

    private:
        winrt::Microsoft::UI::Xaml::Window window{ nullptr };
    };
}
