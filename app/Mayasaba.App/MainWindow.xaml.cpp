// Mayasaba Control Room shell — main window implementation.
//
// The window is chrome only: it extends the content into the title bar so the Mayasaba mark can
// sit in the title area, gives the Chat page its host handle (needed only to parent the native
// folder/file pickers), and applies the generated application icon. It owns no product state.
#include "pch.h"

#include "MainWindow.xaml.h"
#if __has_include("MainWindow.g.cpp")
#include "MainWindow.g.cpp"
#endif

#include <winrt/Windows.Graphics.h>

#include <string>

using namespace winrt;
using namespace winrt::Microsoft::UI::Xaml;

namespace
{
    std::wstring ModuleDirectory()
    {
        wchar_t buffer[MAX_PATH]{};
        const DWORD length = ::GetModuleFileNameW(nullptr, buffer, MAX_PATH);
        if (length == 0 || length >= MAX_PATH)
        {
            return {};
        }
        std::wstring path(buffer, length);
        const auto separator = path.find_last_of(L"\\/");
        if (separator == std::wstring::npos)
        {
            return {};
        }
        return path.substr(0, separator);
    }
}

namespace winrt::Mayasaba::App::implementation
{
    MainWindow::MainWindow()
    {
        InitializeComponent();

        // The mark belongs in the title area, so the app draws its own title bar.
        ExtendsContentIntoTitleBar(true);
        SetTitleBar(AppTitleBar());

        // Give the Chat page the host window handle for the native pickers.
        HWND hwnd{nullptr};
        if (auto window_native = this->try_as<::IWindowNative>())
        {
            window_native->get_WindowHandle(&hwnd);
        }
        ChatHost().HostWindow(reinterpret_cast<int64_t>(hwnd));

        try
        {
            AppWindow().Resize(winrt::Windows::Graphics::SizeInt32{1120, 800});
        }
        catch (...)
        {
        }

        // The icon is generated from the same geometry as the in-app mark by
        // app/assets/generate-icon.ps1 and copied next to the executable.
        try
        {
            const std::wstring icon = ModuleDirectory() + L"\\assets\\mayasaba.ico";
            if (::GetFileAttributesW(icon.c_str()) != INVALID_FILE_ATTRIBUTES)
            {
                AppWindow().SetIcon(winrt::hstring(icon));
            }
        }
        catch (...)
        {
        }
    }
}
