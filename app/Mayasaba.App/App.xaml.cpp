// Mayasaba Control Room shell — Application subclass implementation.
#include "pch.h"

#include "App.xaml.h"
#include "MainWindow.xaml.h"

#include <cstdio>
#include <cwchar>
#include <string>

using namespace winrt;
using namespace winrt::Microsoft::UI::Xaml;

namespace
{
    // Startup breadcrumbs. A fatal WinUI startup failure surfaces as a stowed
    // exception (0xc000027b) and the process dies before anything can report why;
    // this best-effort log preserves the real message. Logging never throws into
    // the app and never blocks: one append per event, next to the executable.
    void LogStartup(std::wstring const& line)
    {
        wchar_t module[MAX_PATH]{};
        const DWORD length = ::GetModuleFileNameW(nullptr, module, MAX_PATH);
        if (length == 0 || length >= MAX_PATH)
        {
            return;
        }
        std::wstring path(module, length);
        const auto separator = path.find_last_of(L"\\/");
        if (separator == std::wstring::npos)
        {
            return;
        }
        path = path.substr(0, separator + 1) + L"mayasaba-startup.log";

        FILE* file = nullptr;
        if (_wfopen_s(&file, path.c_str(), L"a, ccs=UTF-8") != 0 || file == nullptr)
        {
            return;
        }
        SYSTEMTIME now{};
        ::GetLocalTime(&now);
        std::fwprintf(file, L"[%04u-%02u-%02u %02u:%02u:%02u] %ls\n",
                      static_cast<unsigned>(now.wYear), static_cast<unsigned>(now.wMonth),
                      static_cast<unsigned>(now.wDay), static_cast<unsigned>(now.wHour),
                      static_cast<unsigned>(now.wMinute), static_cast<unsigned>(now.wSecond),
                      line.c_str());
        std::fclose(file);
    }
}

namespace winrt::Mayasaba::App::implementation
{
    App::App()
    {
        LogStartup(L"App: constructing");
        InitializeComponent();

        // Always on, not debug-only: in Release the debugger break below is compiled
        // out, and this log is the only place the real startup failure is recorded.
        UnhandledException([](IInspectable const&, UnhandledExceptionEventArgs const& e)
        {
            try
            {
                LogStartup(L"UnhandledException: " + std::wstring(e.Message().c_str()));
            }
            catch (...)
            {
                LogStartup(L"UnhandledException: <no message>");
            }
#if defined _DEBUG && !defined DISABLE_XAML_GENERATED_BREAK_ON_UNHANDLED_EXCEPTION
            if (IsDebuggerPresent())
            {
                __debugbreak();
            }
#endif
        });
        LogStartup(L"App: constructed");
    }

    // Every launch opens the native Chat shell immediately. No wizard, no first-message mode.
    void App::OnLaunched(LaunchActivatedEventArgs const&)
    {
        LogStartup(L"OnLaunched: entering");
        try
        {
            window = make<MainWindow>();
            LogStartup(L"OnLaunched: MainWindow constructed");
            window.Activate();
            LogStartup(L"OnLaunched: MainWindow activated");
        }
        catch (winrt::hresult_error const& error)
        {
            wchar_t code[32]{};
            swprintf_s(code, L"0x%08X", static_cast<unsigned>(error.code().value));
            LogStartup(std::wstring(L"OnLaunched: hresult_error ") + code + L": " +
                       std::wstring(error.message().c_str()));
            throw;
        }
        catch (std::exception const& error)
        {
            std::wstring message;
            for (const char* what = error.what(); what != nullptr && *what != '\0'; ++what)
            {
                message.push_back(static_cast<wchar_t>(static_cast<unsigned char>(*what)));
            }
            LogStartup(L"OnLaunched: std::exception: " + message);
            throw;
        }
        catch (...)
        {
            LogStartup(L"OnLaunched: unknown exception");
            throw;
        }
    }
}
