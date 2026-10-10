// Mayasaba Control Room shell — main window declaration.
#pragma once

#include "pch.h"
#include "MainWindow.g.h"

namespace winrt::Mayasaba::App::implementation
{
    struct MainWindow : MainWindowT<MainWindow>
    {
        MainWindow();
    };
}

namespace winrt::Mayasaba::App::factory_implementation
{
    struct MainWindow : MainWindowT<MainWindow, implementation::MainWindow>
    {
    };
}
