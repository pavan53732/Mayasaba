# Mayasaba — top-level build.
#
# Mayasaba is Windows-only (AGENTS.md section 3). The deterministic core builds and tests
# without creating a window; the WinUI 3 Control Room builds through MSBuild under app/.
#
# Source lists use CONFIGURE_DEPENDS globs so that adding a .cpp file does not require editing
# this file; the build re-globs when the tree changes.

cmake_minimum_required(VERSION 3.28)
project(Mayasaba VERSION 0.1.0 LANGUAGES C CXX)

if(NOT WIN32)
  message(FATAL_ERROR "Mayasaba is a Windows-only product; a Linux/macOS build cannot satisfy any acceptance gate (AGENTS.md section 3).")
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE OFF)

option(MAYASABA_BUILD_TESTS "Build the Mayasaba test suite (CTest)." ON)
option(MAYASABA_ASAN "Build supported targets with AddressSanitizer (MSVC /fsanitize=address)." OFF)

if(MSVC)
  add_compile_options(/W4 /permissive- /utf-8 /Zc:__cplusplus /EHsc /MP)
  add_compile_definitions(UNICODE _UNICODE NOMINMAX WIN32_LEAN_AND_MEAN _WIN32_WINNT=0x0A00)
  if(MAYASABA_ASAN)
    add_compile_options(/fsanitize=address)
    add_link_options(/INCREMENTAL:NO)
  endif()
endif()

add_subdirectory(third_party)

add_subdirectory(core)
add_subdirectory(adapters)
add_subdirectory(app)
add_subdirectory(tools)

if(MAYASABA_BUILD_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()

message(STATUS "Mayasaba ${PROJECT_VERSION}: tests=${MAYASABA_BUILD_TESTS} asan=${MAYASABA_ASAN}")
