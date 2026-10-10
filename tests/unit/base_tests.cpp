// Foundation tests: ids, UTF-8 validation, SHA-256 vectors, redaction, path containment.
#include <gtest/gtest.h>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"
#include "test_support.hpp"

using namespace mayasaba;

TEST(Base, IdShapeAndUniqueness) {
    std::string a = NewId("proj");
    std::string b = NewId("proj");
    EXPECT_NE(a, b);
    EXPECT_TRUE(IsValidId(a));
    EXPECT_TRUE(IsValidId(a.substr(5)));  // without prefix
    EXPECT_FALSE(IsValidId("not-an-id"));
    EXPECT_FALSE(IsValidId(""));
    EXPECT_FALSE(IsValidId("proj_00000000-0000-0000-0000-00000000000"));
}

TEST(Base, Sha256KnownVectors) {
    auto empty = Sha256::HexOf(std::string());
    ASSERT_TRUE(empty.ok());
    EXPECT_EQ(empty.value(),
              "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    auto abc = Sha256::HexOf(std::string("abc"));
    ASSERT_TRUE(abc.ok());
    EXPECT_EQ(abc.value(),
              "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

TEST(Base, ChainLinkIsDeterministicAndSensitive) {
    auto link1 = Sha256::ChainLink("", "payload");
    auto link2 = Sha256::ChainLink("", "payload");
    auto link3 = Sha256::ChainLink("", "payload2");
    ASSERT_TRUE(link1.ok());
    ASSERT_TRUE(link2.ok());
    ASSERT_TRUE(link3.ok());
    EXPECT_EQ(link1.value(), link2.value());
    EXPECT_NE(link1.value(), link3.value());
    auto prev = Sha256::HexOf(std::string("prev"));
    ASSERT_TRUE(prev.ok());
    auto chained = Sha256::ChainLink(prev.value(), "payload");
    ASSERT_TRUE(chained.ok());
    EXPECT_NE(chained.value(), link1.value());
    EXPECT_FALSE(Sha256::ChainLink("not-hex", "payload").ok());
}

TEST(Base, Utf8Validation) {
    EXPECT_TRUE(IsValidUtf8("plain ascii"));
    EXPECT_TRUE(IsValidUtf8("caf\xc3\xa9"));
    EXPECT_TRUE(IsValidUtf8("\xf0\x9f\x98\x80"));  // U+1F600
    EXPECT_FALSE(IsValidUtf8("\xC3\x28"));         // bad continuation
    EXPECT_FALSE(IsValidUtf8("\xC0\xAF"));         // overlong
    EXPECT_FALSE(IsValidUtf8("\xED\xA0\x80"));     // surrogate
    EXPECT_FALSE(IsValidUtf8("\xF5\x80\x80\x80")); // > U+10FFFF
}

TEST(Base, RedactionRemovesSecretShapes) {
    std::string text = "key=sk-abcdefghijklmnopqrstuvwxyz123456 and ghp_ABCDEFGHIJKLMNOPQRST "
                       "plus password: hunter2secret and Authorization: Bearer abcdef123456";
    std::string redacted = RedactSecrets(text);
    EXPECT_EQ(redacted.find("sk-abcdefghijklmnopqrstuvwxyz123456"), std::string::npos);
    EXPECT_EQ(redacted.find("ghp_ABCDEFGHIJKLMNOPQRST"), std::string::npos);
    EXPECT_EQ(redacted.find("hunter2secret"), std::string::npos);
    EXPECT_EQ(redacted.find("abcdef123456"), std::string::npos);
    EXPECT_NE(redacted.find("[REDACTED]"), std::string::npos);
    // Ordinary text survives.
    EXPECT_NE(RedactSecrets("just a normal sentence").find("normal sentence"), std::string::npos);
}

TEST(Fs, PathContainmentComponentwise) {
    EXPECT_TRUE(fs::IsPathWithin("C:\\Projects\\App", "C:\\Projects\\App"));
    EXPECT_TRUE(fs::IsPathWithin("C:\\Projects\\App", "C:\\Projects\\App\\src\\main.cpp"));
    EXPECT_TRUE(fs::IsPathWithin("C:\\Projects\\App", "c:\\projects\\app\\SRC"));
    EXPECT_FALSE(fs::IsPathWithin("C:\\Projects\\App", "C:\\Projects\\App2\\file.txt"));
    EXPECT_FALSE(fs::IsPathWithin("C:\\Projects\\App", "C:\\Projects\\Other\\file.txt"));
    EXPECT_FALSE(fs::IsPathWithin("C:\\Projects\\App", "C:\\Projects"));
}

TEST(Fs, RelativeWithinProducesSlashSeparatedRelativePath) {
    auto relative = fs::RelativeWithin("C:\\Projects\\App", "C:\\Projects\\App\\src\\main.cpp");
    ASSERT_TRUE(relative.ok()) << relative.message();
    EXPECT_EQ(relative.value(), "src/main.cpp");
    auto outside = fs::RelativeWithin("C:\\Projects\\App", "C:\\Projects\\Other\\f.txt");
    EXPECT_FALSE(outside.ok());
    EXPECT_EQ(outside.code(), ErrorCode::Denied);
}

TEST(Fs, CanonicalizeAndIdentify) {
    test::ScratchDir scratch;
    test::WriteText(scratch.File("f.txt"), "hello");
    auto canonical = fs::CanonicalizePath(scratch.File("f.txt"));
    ASSERT_TRUE(canonical.ok()) << canonical.message();
    auto identity = fs::IdentifyPath(canonical.value());
    ASSERT_TRUE(identity.ok());
    EXPECT_TRUE(identity.value().exists);
    EXPECT_FALSE(identity.value().is_directory);
    EXPECT_EQ(identity.value().size, 5u);
    // Same file via a relative-ish alias resolves to the same identity.
    auto alias = fs::CanonicalizePath(scratch.File(".\\f.txt"));
    ASSERT_TRUE(alias.ok());
    auto alias_identity = fs::IdentifyPath(alias.value());
    ASSERT_TRUE(alias_identity.ok());
    EXPECT_TRUE(identity.value().SameFile(alias_identity.value()));
}

TEST(Fs, MissingPathReportsNotExists) {
    auto identity = fs::IdentifyPath("C:\\definitely\\missing\\path\\file.txt");
    ASSERT_TRUE(identity.ok());
    EXPECT_FALSE(identity.value().exists);
}
