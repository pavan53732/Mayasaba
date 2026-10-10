// Mayasaba Control Room — the single, audited bridge from UI code to the controller facade.
//
// core/include/mayasaba/controller.hpp is the ONLY API the UI may call. This header exists so
// that every translation unit which needs it includes it the same way, and so that the
// third-party JSON header the facade pulls in cannot make our own /W4 build noisy.
//
// It deliberately exposes nothing else from the core: no storage, no kernel, no adapters, no
// scheduler. If a UI file needs something the facade does not offer, that is an interface
// change request against the controller contract — never a new include.
#pragma once

#include <memory>
#include <string>
#include <vector>

#pragma warning(push, 0)
#include "mayasaba/status.hpp"
#include "mayasaba/controller.hpp"
#pragma warning(pop)
