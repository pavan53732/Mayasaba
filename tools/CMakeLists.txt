# Local verification tools (no hosted pipeline; everything runs on this machine).

file(GLOB_RECURSE MAYASABA_TOOL_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)

foreach(source ${MAYASABA_TOOL_SOURCES})
  get_filename_component(stem ${source} NAME_WE)
  add_executable(${stem} ${source})
  target_link_libraries(${stem} PRIVATE mayasaba_core mayasaba_adapters)
endforeach()
