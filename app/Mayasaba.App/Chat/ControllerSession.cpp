// Mayasaba Control Room — controller session implementation.
#include "pch.h"

#include "ControllerSession.h"

#include <array>
#include <cstddef>

namespace mayasaba::app {

using mayasaba::ErrorCode;
using mayasaba::Status;

namespace {

// Reads a Windows-known environment root. No shell, no process, no registry — a path lookup.
std::string EnvironmentRoot(const wchar_t* name) {
    std::array<wchar_t, 1024> buffer{};
    const DWORD length =
        ::GetEnvironmentVariableW(name, buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0 || length >= buffer.size()) {
        return {};
    }
    return winrt::to_string(winrt::hstring(buffer.data(), length));
}

std::string JoinPath(const std::string& base, const std::string& leaf) {
    if (base.empty()) {
        return {};
    }
    std::string joined = base;
    if (joined.back() != '\\' && joined.back() != '/') {
        joined.push_back('\\');
    }
    joined += leaf;
    return joined;
}

}  // namespace

std::string ControllerSession::DefaultRootUnder(const std::string& leaf) {
    std::string base = EnvironmentRoot(L"LOCALAPPDATA");
    if (base.empty()) {
        base = EnvironmentRoot(L"USERPROFILE");
    }
    return JoinPath(JoinPath(base, "Mayasaba"), leaf);
}

Status ControllerSession::Start(NoticeHandler on_notice) {
    if (controller_) {
        return Status::Ok();
    }

    mayasaba::control::ControllerConfig config;
    config.storage_root = DefaultRootUnder("storage");
    config.workspace_root = DefaultRootUnder("workspace");
    config.recovery_root = DefaultRootUnder("recovery");
    // Agent executable paths are user-owned. The shell starts with none configured and lets the
    // user point at an installed CLI from a warning card ("Locate executable").
    config.agent_executable_paths.clear();

    auto created = mayasaba::control::Controller::Create(config);
    if (!created.ok()) {
        return created.status();
    }

    std::shared_ptr<mayasaba::control::Controller> controller = std::move(created.value());

    const Status initialized = controller->Initialize();
    if (!initialized.ok()) {
        return initialized;
    }

    if (on_notice) {
        controller->SetNoticeCallback(std::move(on_notice));
    }

    controller_ = std::move(controller);
    return Status::Ok();
}

void ControllerSession::Stop() {
    if (controller_) {
        controller_->SetNoticeCallback(nullptr);
        controller_->Shutdown();
        controller_.reset();
    }
}

}  // namespace mayasaba::app
