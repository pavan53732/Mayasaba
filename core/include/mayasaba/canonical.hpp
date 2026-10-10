// Mayasaba canonical serialization (MCB-1). See contracts/CANONICAL.md for the full profile.
#pragma once

#include <string>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"

namespace mayasaba {

// Serializes `value` to MCB-1 canonical bytes. Rejects invalid UTF-8, non-finite numbers,
// floating point values and unsigned integers above INT64_MAX.
Expected<std::string> CanonicalDump(const nlohmann::json& value);

// SHA-256 (lowercase hex) over the MCB-1 bytes.
Expected<std::string> CanonicalDigest(const nlohmann::json& value);

// Maximum nesting accepted by CanonicalDump.
constexpr int kCanonicalMaxDepth = 128;

}  // namespace mayasaba
