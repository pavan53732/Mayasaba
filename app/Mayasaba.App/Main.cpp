// Mayasaba Control Room shell — process entry point.
//
// This is the readable, checked-in wWinMain: it initialises the Windows Runtime on a
// single-threaded apartment (required by WinUI 3) and hands control to
// Microsoft::UI::Xaml::Application::Start, which drives the XAML message loop and invokes
// App::OnLaunched. The Mayasaba deterministic core is never constructed here.

#include "pch.h"

#include "App.xaml.h"

using namespace winrt;
using namespace winrt::Microsoft::UI::Xaml;

int __stdcall wWinMain(HINSTANCE, HINSTANCE, PWSTR, int)
{
    winrt::init_apartment(winrt::apartment_type::single_threaded);

    Application::Start([](ApplicationInitializationCallbackParams const&)
    {
        make<winrt::Mayasaba::App::implementation::App>();
    });

    return 0;
}
