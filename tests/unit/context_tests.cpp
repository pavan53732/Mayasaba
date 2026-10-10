// Context Synchronizer tests: snapshots, staleness, bounded content, secret filtering.
#include <gtest/gtest.h>

#include <algorithm>
#include <string>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/context.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/workspace.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::context;
using mayasaba::test::ScratchDir;
using mayasaba::test::WriteText;

namespace {

bool Contains(const std::vector<std::string>& values, const std::string& needle) {
    return std::find(values.begin(), values.end(), needle) != values.end();
}

std::size_t DeliveredBytes(const std::vector<ContentReference>& refs) {
    std::size_t total = 0;
    for (const auto& ref : refs) total += ref.content.size();
    return total;
}

}  // namespace

TEST(ContextSnapshot, CreateAndLoadRoundTrip) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "src\\a.cpp"), "int a();");
    WriteText(fs::JoinPath(root, "readme.md"), "# hi");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok()) << manifest.message();
    Synchronizer sync(store.value().get(), root);

    auto created = sync.CreateSnapshot("proj", 3, manifest.value());
    ASSERT_TRUE(created.ok()) << created.message();
    auto loaded = sync.LoadSnapshot(created.value().snapshot_id);
    ASSERT_TRUE(loaded.ok()) << loaded.message();

    EXPECT_EQ(loaded.value().snapshot_id, created.value().snapshot_id);
    EXPECT_EQ(loaded.value().project_id, "proj");
    EXPECT_EQ(loaded.value().epoch, 3);
    EXPECT_EQ(loaded.value().kind, "REPOSITORY");
    EXPECT_EQ(loaded.value().manifest_digest, manifest.value().digest);
    EXPECT_EQ(loaded.value().digest, created.value().digest);
    ASSERT_EQ(loaded.value().entries.size(), created.value().entries.size());
    for (std::size_t i = 0; i < created.value().entries.size(); ++i) {
        EXPECT_EQ(loaded.value().entries[i].rel_path, created.value().entries[i].rel_path);
        EXPECT_EQ(loaded.value().entries[i].sha256, created.value().entries[i].sha256);
        EXPECT_EQ(loaded.value().entries[i].size, created.value().entries[i].size);
    }
}

TEST(ContextSnapshot, DigestStableForSameManifestAndChangesWithEntryHash) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "a.txt"), "aaa");
    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);

    auto first = sync.CreateSnapshot("proj", 0, manifest.value());
    auto second = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(first.ok());
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(first.value().digest, second.value().digest);
    EXPECT_NE(first.value().snapshot_id, second.value().snapshot_id);

    workspace::Manifest mutated = manifest.value();
    for (auto& entry : mutated.entries) {
        if (entry.rel_path == "a.txt") entry.sha256 = std::string(64, 'f');
    }
    auto third = sync.CreateSnapshot("proj", 0, mutated);
    ASSERT_TRUE(third.ok());
    EXPECT_NE(third.value().digest, first.value().digest);
}

TEST(ContextStale, ReportsModifiedAndDeletedEntries) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "keep.txt"), "keep");
    WriteText(fs::JoinPath(root, "modify.txt"), "before");
    WriteText(fs::JoinPath(root, "delete.txt"), "gone");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    WriteText(fs::JoinPath(root, "modify.txt"), "after");
    ASSERT_TRUE(fs::RemoveFile(fs::JoinPath(root, "delete.txt")).ok());

    auto stale = sync.StaleEntries(snapshot.value());
    ASSERT_TRUE(stale.ok()) << stale.message();
    EXPECT_TRUE(Contains(stale.value(), "modify.txt"));
    EXPECT_TRUE(Contains(stale.value(), "delete.txt"));
    EXPECT_FALSE(Contains(stale.value(), "keep.txt"));
}

TEST(ContextContent, BoundedTruncation) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "a.txt"), "0123456789");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    auto ref = sync.ReadContent(snapshot.value(), "a.txt", 4);
    ASSERT_TRUE(ref.ok()) << ref.message();
    EXPECT_TRUE(ref.value().truncated);
    EXPECT_EQ(ref.value().content, "0123");
    EXPECT_TRUE(ref.value().unavailable_reason.empty());
}

TEST(ContextContent, MissingFileReturnsUnavailableReason) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "a.txt"), "aaa");
    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    ASSERT_TRUE(fs::RemoveFile(fs::JoinPath(root, "a.txt")).ok());
    auto ref = sync.ReadContent(snapshot.value(), "a.txt");
    ASSERT_TRUE(ref.ok()) << ref.message();
    EXPECT_FALSE(ref.value().unavailable_reason.empty());
    EXPECT_TRUE(ref.value().content.empty());
}

TEST(ContextContent, SecretShapedContentIsWithheld) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "key.env"), "OPENAI_API_KEY=sk-abc123456789\n");
    WriteText(fs::JoinPath(root, "pw.txt"), "password = \"hunter2\"\n");
    WriteText(fs::JoinPath(root, "code.cpp"), "int add(int a, int b) { return a + b; }\n");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    auto env = sync.ReadContent(snapshot.value(), "key.env");
    ASSERT_TRUE(env.ok());
    EXPECT_TRUE(env.value().content.empty());
    EXPECT_FALSE(env.value().unavailable_reason.empty());

    auto pw = sync.ReadContent(snapshot.value(), "pw.txt");
    ASSERT_TRUE(pw.ok());
    EXPECT_TRUE(pw.value().content.empty());
    EXPECT_FALSE(pw.value().unavailable_reason.empty());

    // Negative case: ordinary code text is delivered unchanged.
    auto code = sync.ReadContent(snapshot.value(), "code.cpp");
    ASSERT_TRUE(code.ok());
    EXPECT_TRUE(code.value().unavailable_reason.empty());
    EXPECT_EQ(code.value().content, "int add(int a, int b) { return a + b; }\n");
}

TEST(ContextContent, ShouldWithholdContentDirect) {
    std::string reason;
    EXPECT_FALSE(ShouldWithholdContent("", &reason));
    EXPECT_FALSE(ShouldWithholdContent("plain documentation text", &reason));
    EXPECT_FALSE(ShouldWithholdContent("int main() { return 0; }", &reason));
    EXPECT_TRUE(ShouldWithholdContent("password = \"hunter2\"", &reason));
    EXPECT_FALSE(reason.empty());
    EXPECT_TRUE(ShouldWithholdContent("OPENAI_API_KEY=sk-abc123456789", &reason));
}

TEST(ContextTask, PrefixFilterAndBudgetRespected) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "src\\a.txt"), "aaaaaaaaaa");
    WriteText(fs::JoinPath(root, "src\\b.txt"), "bbbbbbbbbb");
    WriteText(fs::JoinPath(root, "lib\\c.txt"), "cccccccccc");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    auto delivered = sync.ReadTaskContext(snapshot.value(), {"src"}, 15);
    ASSERT_TRUE(delivered.ok()) << delivered.message();
    ASSERT_EQ(delivered.value().size(), 2u);
    for (const auto& ref : delivered.value()) {
        EXPECT_EQ(ref.rel_path.rfind("src/", 0), 0u);
    }
    EXPECT_LE(DeliveredBytes(delivered.value()), 15u + 10u);
}

TEST(ContextTask, TamperedEntryIsReportedNotServed) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "src\\a.txt"), "original-content");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    WriteText(fs::JoinPath(root, "src\\a.txt"), "tampered-different-content");

    auto delivered = sync.ReadTaskContext(snapshot.value(), {"src/a.txt"}, 1u << 20);
    ASSERT_TRUE(delivered.ok()) << delivered.message();
    ASSERT_EQ(delivered.value().size(), 1u);
    EXPECT_TRUE(delivered.value()[0].content.empty());
    EXPECT_FALSE(delivered.value()[0].unavailable_reason.empty());
}

TEST(ContextDeps, IncludeScanFindsEdges) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "main.cpp"), "#include \"a.h\"\nint main() { return 0; }\n");
    WriteText(fs::JoinPath(root, "a.h"), "#pragma once\n");

    auto manifest = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snapshot.ok());

    auto ref = sync.ReadContent(snapshot.value(), "main.cpp");
    ASSERT_TRUE(ref.ok());
    ASSERT_FALSE(ref.value().content.empty());

    auto edges = sync.AdvisoryDependencies(snapshot.value(), {ref.value()});
    ASSERT_EQ(edges.size(), 1u);
    EXPECT_EQ(edges[0].from_path, "main.cpp");
    EXPECT_EQ(edges[0].to_path, "a.h");
    EXPECT_EQ(edges[0].method, "include-scan");
    EXPECT_TRUE(edges[0].uncertain);
}

TEST(ContextInvalidation, ChangedFileAndDependentsAreReturned) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "a.h"), "v1");
    WriteText(fs::JoinPath(root, "b.h"), "v1");
    WriteText(fs::JoinPath(root, "main.cpp"), "#include \"a.h\"\n");

    auto manifest1 = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest1.ok());
    Synchronizer sync(store.value().get(), root);
    auto snapshot = sync.CreateSnapshot("proj", 0, manifest1.value());
    ASSERT_TRUE(snapshot.ok());

    WriteText(fs::JoinPath(root, "a.h"), "v2-changed");
    auto manifest2 = workspace::BuildManifest("proj", root);
    ASSERT_TRUE(manifest2.ok());

    auto invalidated = sync.InvalidatedPaths(snapshot.value(), manifest2.value());
    ASSERT_TRUE(invalidated.ok()) << invalidated.message();
    EXPECT_TRUE(Contains(invalidated.value(), "a.h"));
    EXPECT_TRUE(Contains(invalidated.value(), "main.cpp"));
    EXPECT_FALSE(Contains(invalidated.value(), "b.h"));
}
