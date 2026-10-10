// Contract tests: MCB-1 canonical serialization profile (contracts/CANONICAL.md).
#include <gtest/gtest.h>

#include "mayasaba/canonical.hpp"
#include "mayasaba/jsonio.hpp"

using mayasaba::CanonicalDigest;
using mayasaba::CanonicalDump;
using mayasaba::ErrorCode;
using mayasaba::ParseJsonBounded;

TEST(Canonical, KeyOrderingIsByteLexicographic) {
    auto value = nlohmann::json::parse(R"({"b":1,"a":2,"B":3})");
    auto dumped = CanonicalDump(value);
    ASSERT_TRUE(dumped.ok()) << dumped.message();
    EXPECT_EQ(dumped.value(), R"({"B":3,"a":2,"b":1})");
}

TEST(Canonical, NestedStructuresDeterministic) {
    auto value = nlohmann::json::parse(R"({"z":[{"y":1,"x":2}],"a":{"d":true,"c":null}})");
    auto dumped = CanonicalDump(value);
    ASSERT_TRUE(dumped.ok()) << dumped.message();
    EXPECT_EQ(dumped.value(), R"({"a":{"c":null,"d":true},"z":[{"x":2,"y":1}]})");
}

TEST(Canonical, NoWhitespaceAndMinimalEscapes) {
    auto value = nlohmann::json::parse(R"({"text":"line\nbreak\tand \"quotes\" \u0001"})");
    auto dumped = CanonicalDump(value);
    ASSERT_TRUE(dumped.ok()) << dumped.message();
    EXPECT_EQ(dumped.value(), R"({"text":"line\nbreak\tand \"quotes\" \u0001"})");
}

TEST(Canonical, UnicodeEmittedLiterallyAndOrderedByUtf8Bytes) {
    // "é" (U+00E9) encodes as 0xC3 0xA9; "z" is 0x7A; so "z" sorts before "é".
    auto value = nlohmann::json::parse(R"({"é":"café","z":"naïve"})");
    auto dumped = CanonicalDump(value);
    ASSERT_TRUE(dumped.ok()) << dumped.message();
    EXPECT_EQ(dumped.value(), "{\"z\":\"na\xc3\xafve\",\"\xc3\xa9\":\"caf\xc3\xa9\"}");
}

TEST(Canonical, IntegerBoundaries) {
    nlohmann::json value;
    value["max"] = std::numeric_limits<std::int64_t>::max();
    value["min"] = std::numeric_limits<std::int64_t>::min();
    auto dumped = CanonicalDump(value);
    ASSERT_TRUE(dumped.ok()) << dumped.message();
    EXPECT_EQ(dumped.value(), R"({"max":9223372036854775807,"min":-9223372036854775808})");
}

TEST(Canonical, RejectsFloatingPoint) {
    auto value = nlohmann::json::parse(R"({"ratio":0.5})");
    auto dumped = CanonicalDump(value);
    ASSERT_FALSE(dumped.ok());
    EXPECT_EQ(dumped.code(), ErrorCode::SchemaViolation);
}

TEST(Canonical, RejectsUnsignedOverflow) {
    nlohmann::json value;
    value["big"] = std::numeric_limits<std::uint64_t>::max();
    auto dumped = CanonicalDump(value);
    ASSERT_FALSE(dumped.ok());
    EXPECT_EQ(dumped.code(), ErrorCode::SchemaViolation);
}

TEST(Canonical, RejectsInvalidUtf8) {
    nlohmann::json value;
    value["bad"] = std::string("\xC3\x28");  // invalid 2-byte sequence
    auto dumped = CanonicalDump(value);
    ASSERT_FALSE(dumped.ok());
    EXPECT_EQ(dumped.code(), ErrorCode::SchemaViolation);
}

TEST(Canonical, DigestIsStableAcrossRuns) {
    auto value = nlohmann::json::parse(R"({"b":1,"a":[1,2,3],"s":"x"})");
    auto first = CanonicalDigest(value);
    auto second = CanonicalDigest(value);
    ASSERT_TRUE(first.ok());
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(first.value(), second.value());
    EXPECT_EQ(first.value().size(), 64u);
    // A known vector computed from the MCB-1 bytes.
    auto bytes = CanonicalDump(value);
    ASSERT_TRUE(bytes.ok());
    EXPECT_EQ(bytes.value(), R"({"a":[1,2,3],"b":1,"s":"x"})");
}

TEST(JsonBounded, RejectsDeepNestingBeforeRecursion) {
    std::string hostile;
    hostile.reserve(5000);
    for (int i = 0; i < 2000; ++i) hostile.push_back('[');
    auto parsed = ParseJsonBounded(hostile);
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.code(), ErrorCode::InvalidJson);
}

TEST(JsonBounded, RejectsOversizedDocument) {
    std::string big = "{\"data\":\"" + std::string(2 * 1024 * 1024, 'x') + "\"}";
    auto parsed = ParseJsonBounded(big);
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.code(), ErrorCode::InvalidJson);
}

TEST(JsonBounded, RejectsTrailingGarbageAndInvalidUtf8) {
    auto trailing = ParseJsonBounded(R"({"a":1} trailing)");
    EXPECT_FALSE(trailing.ok());
    std::string invalid_utf8 = "{\"a\":\"";
    invalid_utf8.push_back('\xC3');
    invalid_utf8.push_back('\x28');
    invalid_utf8 += "\"}";
    auto bad = ParseJsonBounded(invalid_utf8);
    EXPECT_FALSE(bad.ok());
}

TEST(JsonBounded, AcceptsValidDocument) {
    auto parsed = ParseJsonBounded(R"({"a":1,"b":["x"]})");
    ASSERT_TRUE(parsed.ok()) << parsed.message();
    EXPECT_EQ(parsed.value()["a"], 1);
}
