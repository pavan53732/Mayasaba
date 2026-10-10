# Controller Application Services (layer 2): the Orchestrator and facade.
# Depends on core (layers 3, 5-13) and adapters (layer 4). This library owns cross-service
# scheduling, barriers, registered decision triggers and phase transitions. It does NOT
# mutate another service's authoritative records. Lower layers never depend on this.
file(GLOB_RECURSE MAYASABA_APP_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)

add_library(mayasaba_app STATIC ${MAYASABA_APP_SOURCES})
target_include_directories(mayasaba_app PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_link_libraries(mayasaba_app
  PUBLIC mayasaba_core mayasaba_adapters
  PRIVATE mayasaba_json)
