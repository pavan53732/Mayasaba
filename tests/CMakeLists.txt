# Test suite. Every *_tests.cpp file becomes one CTest test target.

file(GLOB_RECURSE MAYASABA_TEST_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_CURRENT_SOURCE_DIR}/unit/*_tests.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/contract/*_tests.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/integration/*_tests.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/fault/*_tests.cpp
  ${CMAKE_CURRENT_SOURCE_DIR}/e2e/*_tests.cpp)

set(MAYASABA_TEST_TARGETS "")

foreach(source ${MAYASABA_TEST_SOURCES})
  get_filename_component(stem ${source} NAME_WE)
  add_executable(${stem} ${source})
  target_link_libraries(${stem} PRIVATE
    mayasaba_core mayasaba_adapters mayasaba_app GTest::gtest GTest::gmock GTest::gtest_main)
  target_include_directories(${stem} PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
  target_compile_definitions(${stem} PRIVATE MAYASABA_SOURCE_DIR="${CMAKE_SOURCE_DIR}")
  add_test(NAME ${stem} COMMAND ${stem})
  list(APPEND MAYASABA_TEST_TARGETS ${stem})
endforeach()

# Helper executables used by process/adapter tests (scripted CLI, child-process tree).
file(GLOB_RECURSE MAYASABA_HELPER_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_CURRENT_SOURCE_DIR}/helpers/*.cpp)
foreach(source ${MAYASABA_HELPER_SOURCES})
  get_filename_component(stem ${source} NAME_WE)
  add_executable(${stem} ${source})
  # The containment probe must be runnable with only the restricted workspace
  # readable.  Keep this helper self-contained so it does not need developer-CRT
  # DLLs or Mayasaba libraries outside that workspace.
  if(stem STREQUAL "child_process_helper")
    set_property(TARGET ${stem} PROPERTY
      MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
  else()
    target_link_libraries(${stem} PRIVATE mayasaba_core mayasaba_adapters)
  endif()
  set_target_properties(${stem} PROPERTIES FOLDER "tests/helpers")
endforeach()

if(MSVC AND MAYASABA_ASAN)
  get_filename_component(MSVC_BIN_DIR "${CMAKE_CXX_COMPILER}" DIRECTORY)
  foreach(target ${MAYASABA_TEST_TARGETS})
    set_property(TEST ${target} PROPERTY ENVIRONMENT "PATH=${MSVC_BIN_DIR};$ENV{PATH}")
  endforeach()
  if(EXISTS "${MSVC_BIN_DIR}/clang_rt.asan_dbg_dynamic-x86_64.dll")
    file(COPY "${MSVC_BIN_DIR}/clang_rt.asan_dbg_dynamic-x86_64.dll"
              "${MSVC_BIN_DIR}/clang_rt.asan_dynamic-x86_64.dll"
         DESTINATION "${CMAKE_CURRENT_BINARY_DIR}/Debug")
    file(COPY "${MSVC_BIN_DIR}/clang_rt.asan_dbg_dynamic-x86_64.dll"
              "${MSVC_BIN_DIR}/clang_rt.asan_dynamic-x86_64.dll"
         DESTINATION "${CMAKE_CURRENT_BINARY_DIR}/Release")
  endif()
endif()

message(STATUS "Mayasaba test targets: ${MAYASABA_TEST_TARGETS}")
