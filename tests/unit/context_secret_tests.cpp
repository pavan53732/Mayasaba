#include <gtest/gtest.h>

#include <string>

#include "mayasaba/context.hpp"

TEST(ContextSecretAdmission, RejectsBearerCredentialsCaseInsensitively) {
    std::string reason;
    EXPECT_TRUE(mayasaba::context::ShouldWithholdContent(
        "Authorization: BEARER synthetic_test_token_0123456789", &reason));
    EXPECT_NE(reason.find("bearer token"), std::string::npos);
    EXPECT_EQ(reason.find("synthetic_test_token"), std::string::npos);
}

TEST(ContextSecretAdmission, RejectsDatabaseUriCredentials) {
    std::string reason;
    EXPECT_TRUE(mayasaba::context::ShouldWithholdContent(
        "postgresql://synthetic:fake_password@localhost/test", &reason));
    EXPECT_NE(reason.find("connection string URI"), std::string::npos);
    EXPECT_EQ(reason.find("fake_password"), std::string::npos);
}

TEST(ContextSecretAdmission, RejectsClientSecretsAndAuthenticationTokens) {
    for (const std::string value : {
             "client_secret = 'synthetic_client_value_12345'",
             "auth-token = synthetic_auth_value_12345"}) {
        std::string reason;
        EXPECT_TRUE(mayasaba::context::ShouldWithholdContent(value, &reason));
        EXPECT_FALSE(reason.empty());
        EXPECT_EQ(reason.find("synthetic"), std::string::npos);
    }
}

TEST(ContextSecretAdmission, PreservesOrdinaryDocumentation) {
    for (const std::string value : {"", "Bearer token authentication is documented.",
                                    "Database connection configuration belongs to the user.",
                                    "int main() { return 0; }"}) {
        EXPECT_FALSE(mayasaba::context::ShouldWithholdContent(value, nullptr));
    }
}
