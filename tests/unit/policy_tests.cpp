// Policy Engine tests: default-deny, grant scoping, revocation, persisted denials.
#include <gtest/gtest.h>

#include <string>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/policy.hpp"
#include "mayasaba/store.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::policy;
using mayasaba::test::ScratchDir;

namespace {

policy::ActionRequest ReadRequest(const std::string& project_id, const std::string& task_id,
                                  const std::string& path) {
    policy::ActionRequest request;
    request.kind = policy::ActionKind::ReadPath;
    request.project_id = project_id;
    request.task_id = task_id;
    request.path = path;
    return request;
}

std::string Canonical(const std::string& path) {
    auto canonical = fs::CanonicalizePath(path);
    EXPECT_TRUE(canonical.ok()) << canonical.message();
    return canonical.ok() ? canonical.value() : path;
}

}  // namespace

TEST(Policy, NoGrantsIsDefaultDenyAndRecorded) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok()) << store.message();
    policy::Engine engine(store.value().get());

    auto decision = engine.Authorize(ReadRequest("proj", "task-1", scratch.File("root\\a.txt")));
    EXPECT_FALSE(decision.allowed);
    EXPECT_FALSE(decision.reason.empty());
    EXPECT_EQ(decision.policy_ref, "policy.default-deny");

    auto denials = engine.DenialsFor("proj");
    ASSERT_TRUE(denials.ok()) << denials.message();
    ASSERT_EQ(denials.value().size(), 1u);
    EXPECT_EQ(denials.value()[0]["action"], "ReadPath");
    EXPECT_FALSE(denials.value()[0]["reason"].get<std::string>().empty());
}

TEST(Policy, DenialsSurviveStoreReopen) {
    ScratchDir scratch;
    const std::string db_path = scratch.File("mayasaba.db");
    {
        auto store = storage::Store::Open(db_path);
        ASSERT_TRUE(store.ok());
        policy::Engine engine(store.value().get());
        auto decision = engine.Authorize(ReadRequest("proj", "task-1", scratch.File("x")));
        EXPECT_FALSE(decision.allowed);
    }
    auto reopened = storage::Store::Open(db_path);
    ASSERT_TRUE(reopened.ok());
    policy::Engine engine(reopened.value().get());
    auto denials = engine.DenialsFor("proj");
    ASSERT_TRUE(denials.ok());
    EXPECT_EQ(denials.value().size(), 1u);
}

TEST(Policy, ReadPathScopedToReadableRoots) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    const std::string root = scratch.File("proj");
    ASSERT_TRUE(fs::EnsureDirectory(root).ok());
    const std::string root_canonical = Canonical(root);

    policy::GrantSet grants;
    grants.readable_roots = {root_canonical};
    grants.authority_ref = "user-authorization-1";
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());

    // Inside the readable root: allowed.
    auto allowed = engine.Authorize(ReadRequest("proj", "task-1", fs::JoinPath(root, "src\\a.cpp")));
    EXPECT_TRUE(allowed.allowed) << allowed.reason;
    EXPECT_EQ(allowed.policy_ref, "user-authorization-1");

    // Sibling path with a similar prefix: denied (no prefix confusion).
    auto sibling = engine.Authorize(ReadRequest("proj", "task-1", scratch.File("proj-evil\\a.cpp")));
    EXPECT_FALSE(sibling.allowed);

    // Parent of the root: denied.
    auto parent = engine.Authorize(ReadRequest("proj", "task-1", scratch.path()));
    EXPECT_FALSE(parent.allowed);
}

TEST(Policy, DotDotEscapeAndUncAreDenied) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    const std::string root = scratch.File("proj");
    ASSERT_TRUE(fs::EnsureDirectory(root).ok());
    policy::GrantSet grants;
    grants.readable_roots = {Canonical(root)};
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());

    // C:\proj\..\other must not be authorized even though it starts with the root prefix.
    auto escape = engine.Authorize(ReadRequest("proj", "task-1", fs::JoinPath(root, "..\\other\\x")));
    EXPECT_FALSE(escape.allowed);

    auto unc = engine.Authorize(ReadRequest("proj", "task-1", "\\\\server\\share\\x"));
    EXPECT_FALSE(unc.allowed);
}

TEST(Policy, WriteAndPublishScopedToTheirRoots) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    const std::string readable = scratch.File("read");
    const std::string writable = scratch.File("write");
    const std::string publish = scratch.File("publish");
    ASSERT_TRUE(fs::EnsureDirectory(readable).ok());
    ASSERT_TRUE(fs::EnsureDirectory(writable).ok());
    ASSERT_TRUE(fs::EnsureDirectory(publish).ok());

    policy::GrantSet grants;
    grants.readable_roots = {Canonical(readable)};
    grants.writable_roots = {Canonical(writable)};
    grants.publish_roots = {Canonical(publish)};
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());

    policy::ActionRequest write;
    write.kind = policy::ActionKind::WritePath;
    write.project_id = "proj";
    write.task_id = "task-1";
    write.path = fs::JoinPath(writable, "out.txt");
    EXPECT_TRUE(engine.Authorize(write).allowed);

    // A write into the readable-only root is denied.
    policy::ActionRequest bad_write = write;
    bad_write.path = fs::JoinPath(readable, "out.txt");
    EXPECT_FALSE(engine.Authorize(bad_write).allowed);

    policy::ActionRequest publish_request;
    publish_request.kind = policy::ActionKind::PublishFile;
    publish_request.project_id = "proj";
    publish_request.task_id = "task-1";
    publish_request.path = fs::JoinPath(publish, "final.txt");
    EXPECT_TRUE(engine.Authorize(publish_request).allowed);

    policy::ActionRequest bad_publish = publish_request;
    bad_publish.path = fs::JoinPath(writable, "final.txt");
    EXPECT_FALSE(engine.Authorize(bad_publish).allowed);
}

TEST(Policy, BooleanCapabilityGates) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    policy::GrantSet grants;  // all booleans false
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());

    for (policy::ActionKind kind :
         {policy::ActionKind::LaunchProcess, policy::ActionKind::AgentSession,
          policy::ActionKind::NativeWebTool}) {
        policy::ActionRequest request;
        request.kind = kind;
        request.project_id = "proj";
        request.task_id = "task-1";
        EXPECT_FALSE(engine.Authorize(request).allowed) << policy::ActionKindName(kind);
    }

    policy::GrantSet enabled;
    enabled.allow_process_launch = true;
    enabled.allow_agent_session = true;
    enabled.allow_native_web_tools = true;
    ASSERT_TRUE(engine.Grant("task-2", enabled).ok());
    for (policy::ActionKind kind :
         {policy::ActionKind::LaunchProcess, policy::ActionKind::AgentSession,
          policy::ActionKind::NativeWebTool}) {
        policy::ActionRequest request;
        request.kind = kind;
        request.project_id = "proj";
        request.task_id = "task-2";
        EXPECT_TRUE(engine.Authorize(request).allowed) << policy::ActionKindName(kind);
    }
}

TEST(Policy, EmptySubjectKeyDenied) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    policy::ActionRequest request;  // no project_id, no task_id
    request.kind = policy::ActionKind::ReadPath;
    request.path = scratch.File("x");
    auto decision = engine.Authorize(request);
    EXPECT_FALSE(decision.allowed);
    EXPECT_NE(decision.reason.find("subject"), std::string::npos);
}

TEST(Policy, MaterialActionRequiresOperationId) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());
    policy::GrantSet grants;
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());

    policy::ActionRequest request;
    request.kind = policy::ActionKind::MaterialAction;
    request.project_id = "proj";
    request.task_id = "task-1";
    EXPECT_FALSE(engine.Authorize(request).allowed);

    request.operation_id = NewId("op");
    EXPECT_TRUE(engine.Authorize(request).allowed);
}

TEST(Policy, RevokeRemovesGrants) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    const std::string root = scratch.File("proj");
    ASSERT_TRUE(fs::EnsureDirectory(root).ok());
    policy::GrantSet grants;
    grants.readable_roots = {Canonical(root)};
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());
    EXPECT_TRUE(engine.Authorize(ReadRequest("proj", "task-1", fs::JoinPath(root, "a"))).allowed);

    ASSERT_TRUE(engine.Revoke("task-1").ok());
    EXPECT_FALSE(engine.GrantsFor("task-1").has_value());
    EXPECT_FALSE(engine.Authorize(ReadRequest("proj", "task-1", fs::JoinPath(root, "a"))).allowed);
}

TEST(Policy, RequireReturnsDeniedStatus) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    auto status = engine.Require(ReadRequest("proj", "task-1", scratch.File("a")));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.code(), ErrorCode::Denied);

    const std::string root = scratch.File("proj");
    ASSERT_TRUE(fs::EnsureDirectory(root).ok());
    policy::GrantSet grants;
    grants.readable_roots = {Canonical(root)};
    ASSERT_TRUE(engine.Grant("task-1", grants).ok());
    EXPECT_TRUE(engine.Require(ReadRequest("proj", "task-1", fs::JoinPath(root, "a"))).ok());
}

TEST(Policy, RecordDenialPersistsExplicitDenial) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    policy::Engine engine(store.value().get());

    ASSERT_TRUE(engine
                    .RecordDenial("proj", policy::ActionKind::PublishFile, {{"rel_path", "x"}},
                                  "publication refused: reparse escape")
                    .ok());
    auto denials = engine.DenialsFor("proj");
    ASSERT_TRUE(denials.ok());
    ASSERT_EQ(denials.value().size(), 1u);
    EXPECT_EQ(denials.value()[0]["action"], "PublishFile");
    EXPECT_EQ(denials.value()[0]["subject"]["rel_path"], "x");
}
