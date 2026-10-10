#include "mayasaba/jsonio.hpp"

#include "mayasaba/base.hpp"

namespace mayasaba {
namespace {

// Linear pre-scan for maximum bracket nesting outside string literals. This runs before the
// recursive parser so hostile "[[[[..." input cannot exhaust the stack.
Expected<int> ScanDepth(const std::string& text) {
    int depth = 0;
    int max_depth = 0;
    bool in_string = false;
    bool escaped = false;
    for (char c : text) {
        if (in_string) {
            if (escaped) {
                escaped = false;
            } else if (c == '\\') {
                escaped = true;
            } else if (c == '"') {
                in_string = false;
            }
            continue;
        }
        switch (c) {
            case '"': in_string = true; break;
            case '{':
            case '[':
                ++depth;
                if (depth > max_depth) max_depth = depth;
                break;
            case '}':
            case ']':
                --depth;
                if (depth < 0) return 0;  // malformed; the parser will report it
                break;
            default: break;
        }
    }
    return max_depth;
}

Expected<bool> CheckCollections(const nlohmann::json& value, const JsonLimits& limits, int depth) {
    if (depth > limits.max_depth) {
        return Fail<bool>(ErrorCode::InvalidJson, "JSON nesting exceeds limit");
    }
    if (value.is_array() || value.is_object()) {
        if (value.size() > limits.max_collection) {
            return Fail<bool>(ErrorCode::InvalidJson, "JSON collection exceeds size limit");
        }
        for (const auto& element : value) {
            auto status = CheckCollections(element, limits, depth + 1);
            if (!status.ok()) return status;
        }
    }
    return true;
}

}  // namespace

Expected<nlohmann::json> ParseJsonBounded(const std::string& text, const JsonLimits& limits) {
    if (text.size() > limits.max_bytes) {
        return Fail<nlohmann::json>(ErrorCode::InvalidJson, "JSON document exceeds byte limit");
    }
    if (!IsValidUtf8(text)) {
        return Fail<nlohmann::json>(ErrorCode::InvalidJson, "JSON document is not valid UTF-8");
    }
    auto depth = ScanDepth(text);
    if (!depth.ok()) return Expected<nlohmann::json>(depth.status());
    if (depth.value() > limits.max_depth) {
        return Fail<nlohmann::json>(ErrorCode::InvalidJson, "JSON nesting exceeds limit (pre-scan)");
    }
    nlohmann::json parsed = nlohmann::json::parse(text, nullptr, false);
    if (parsed.is_discarded()) {
        return Fail<nlohmann::json>(ErrorCode::InvalidJson, "JSON parse failed");
    }
    auto checked = CheckCollections(parsed, limits, 0);
    if (!checked.ok()) return Expected<nlohmann::json>(checked.status());
    return parsed;
}

std::string DumpCompact(const nlohmann::json& value) {
    return value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
}

}  // namespace mayasaba
