# Third-party dependencies. Everything here is vendored in-tree and pinned by hash in
# deps/manifest.json; the build performs no network access.

# --- SQLite amalgamation (public domain) -------------------------------------------------
add_library(mayasaba_sqlite3 STATIC sqlite/sqlite3.c)
target_include_directories(mayasaba_sqlite3 PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/sqlite)
target_compile_definitions(mayasaba_sqlite3 PRIVATE
  SQLITE_THREADSAFE=1
  SQLITE_OMIT_LOAD_EXTENSION
  SQLITE_DQS=0
  SQLITE_DEFAULT_FOREIGN_KEYS=1
  SQLITE_ENABLE_COLUMN_METADATA
  SQLITE_USE_URI=0
)
if(MSVC)
  # The amalgamation is C; keep its warning level lower than our own code.
  target_compile_options(mayasaba_sqlite3 PRIVATE /W0)
  # Silence the CRT "unsafe" deprecations inside the amalgamation only.
  target_compile_definitions(mayasaba_sqlite3 PRIVATE _CRT_SECURE_NO_WARNINGS)
endif()

# --- nlohmann/json (MIT), header only ----------------------------------------------------
add_library(mayasaba_json INTERFACE)
target_include_directories(mayasaba_json INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})

# --- GoogleTest (BSD-3-Clause) -----------------------------------------------------------
if(MAYASABA_BUILD_TESTS)
  set(gtest_force_shared_crt ON CACHE BOOL "" FORCE)
  set(BUILD_GMOCK ON CACHE BOOL "" FORCE)
  set(INSTALL_GTEST OFF CACHE BOOL "" FORCE)
  set(GTEST_HAS_ABSL OFF CACHE BOOL "" FORCE)
  add_subdirectory(googletest EXCLUDE_FROM_ALL)
endif()
