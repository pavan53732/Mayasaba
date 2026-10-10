# Deterministic core (layers 2, 3, 5-13). WinUI-independent: builds and tests without a window.

file(GLOB_RECURSE MAYASABA_CORE_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)

add_library(mayasaba_core STATIC ${MAYASABA_CORE_SOURCES})
target_include_directories(mayasaba_core PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_link_libraries(mayasaba_core
  PUBLIC mayasaba_json
  PRIVATE mayasaba_sqlite3 bcrypt ole32 advapi32 user32)

if(MSVC)
  target_compile_options(mayasaba_core PRIVATE /W4)
endif()
