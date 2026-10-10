// Mayasaba Control Room — plain C++ controller session owner.
//
// Creates the one controller the UI is allowed to talk to, supplies its Mayasaba-owned storage
// roots, and owns its lifetime. The UI never constructs storage, adapters or the execution
// kernel directly; it hands the controller three directories and asks it to initialise itself.
#pragma once

#include <functional>
#include <memory>
#include <string>

#include "ControllerBridge.h"

namespace mayasaba::app {

class ControllerSession {
public:
    // Called from a controller-owned thread when the timeline changes. The page marshals this
    // onto the UI thread before re-querying.
    using NoticeHandler = std::function<void()>;

    // Creates and initialises the controller. Returns the controller's own failure reason when
    // it cannot start, so the shell can show an honest error state instead of a blank window.
    mayasaba::Status Start(NoticeHandler on_notice);

    // Stops background pumping and releases the controller. Safe to call more than once.
    void Stop();

    std::shared_ptr<mayasaba::control::Controller> Controller() const { return controller_; }
    bool Started() const { return controller_ != nullptr; }

    // %LOCALAPPDATA%\Mayasaba\<leaf>, or the user profile fallback. Path resolution only.
    static std::string DefaultRootUnder(const std::string& leaf);

private:
    std::shared_ptr<mayasaba::control::Controller> controller_;
};

}  // namespace mayasaba::app
