#include "mayasaba/canonical.hpp"

#include <algorithm>
#include <vector>

#include "mayasaba/base.hpp"

namespace mayasaba {
namespace {

void AppendEscapedString(const std::string& s, std::string& out) {
    out.push_back('"');
    for (unsigned char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\b': out += "\\b"; break;
            case '\f': out += "\\f"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else {
                    out.push_back(static_cast<char>(c));
                }
        }
    }
    out.push_back('"');
}

Expected<bool> DumpValue(const nlohmann::json& value, std::string& out, int depth) {
    if (depth > kCanonicalMaxDepth) {
        return Fail<bool>(ErrorCode::SchemaViolation, "MCB-1: nesting exceeds limit");
    }
    switch (value.type()) {
        case nlohmann::json::value_t::null:
            out += "null";
            return true;
        case nlohmann::json::value_t::boolean:
            out += value.get<bool>() ? "true" : "false";
            return true;
        case nlohmann::json::value_t::string: {
            const std::string& s = value.get_ref<const std::string&>();
            if (!IsValidUtf8(s)) {
                return Fail<bool>(ErrorCode::SchemaViolation, "MCB-1: invalid UTF-8 string");
            }
            AppendEscapedString(s, out);
            return true;
        }
        case nlohmann::json::value_t::number_integer:
        case nlohmann::json::value_t::number_unsigned: {
            if (value.is_number_unsigned() &&
                value.get<std::uint64_t>() > static_cast<std::uint64_t>(INT64_MAX)) {
                return Fail<bool>(ErrorCode::SchemaViolation,
                                  "MCB-1: unsigned integer exceeds INT64_MAX");
            }
            out += std::to_string(value.get<std::int64_t>());
            return true;
        }
        case nlohmann::json::value_t::number_float:
            return Fail<bool>(ErrorCode::SchemaViolation,
                              "MCB-1: floating point values are rejected in hashed payloads");
        case nlohmann::json::value_t::object: {
            std::vector<std::string> keys;
            keys.reserve(value.size());
            for (auto it = value.begin(); it != value.end(); ++it) keys.push_back(it.key());
            std::sort(keys.begin(), keys.end());  // byte-wise lexicographic (std::string <)
            out.push_back('{');
            bool first = true;
            for (const auto& key : keys) {
                if (!IsValidUtf8(key)) {
                    return Fail<bool>(ErrorCode::SchemaViolation, "MCB-1: invalid UTF-8 key");
                }
                if (!first) out.push_back(',');
                first = false;
                AppendEscapedString(key, out);
                out.push_back(':');
                auto status = DumpValue(value.at(key), out, depth + 1);
                if (!status.ok()) return status;
            }
            out.push_back('}');
            return true;
        }
        case nlohmann::json::value_t::array: {
            out.push_back('[');
            bool first = true;
            for (const auto& element : value) {
                if (!first) out.push_back(',');
                first = false;
                auto status = DumpValue(element, out, depth + 1);
                if (!status.ok()) return status;
            }
            out.push_back(']');
            return true;
        }
        case nlohmann::json::value_t::binary:
        case nlohmann::json::value_t::discarded:
        default:
            return Fail<bool>(ErrorCode::SchemaViolation, "MCB-1: unsupported JSON value kind");
    }
}

}  // namespace

Expected<std::string> CanonicalDump(const nlohmann::json& value) {
    std::string out;
    auto status = DumpValue(value, out, 0);
    if (!status.ok()) return Expected<std::string>(status.status());
    return out;
}

Expected<std::string> CanonicalDigest(const nlohmann::json& value) {
    auto bytes = CanonicalDump(value);
    if (!bytes.ok()) return bytes;
    return Sha256::HexOf(bytes.value());
}

}  // namespace mayasaba
