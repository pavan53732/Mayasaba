# Agent Gateway adapters (layer 4): one adapter per CLI, no native wire format escapes here.

file(GLOB_RECURSE MAYASABA_ADAPTER_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)

add_library(mayasaba_adapters STATIC ${MAYASABA_ADAPTER_SOURCES})
target_include_directories(mayasaba_adapters PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_link_libraries(mayasaba_adapters PUBLIC mayasaba_core)
