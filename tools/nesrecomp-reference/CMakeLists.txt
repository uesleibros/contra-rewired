cmake_minimum_required(VERSION 3.20)
project(ContraRecompExperiment C)

set(CMAKE_C_STANDARD 11)

set(NESRECOMP_ROOT ${CMAKE_SOURCE_DIR}/../nesrecomp)
include(${NESRECOMP_ROOT}/runner/runner.cmake)

set(NESRECOMP_ENABLE_TRACE OFF)
set(NESRECOMP_ENABLE_POSTMORTEM_RINGS OFF)

find_package(SDL2 REQUIRED)

add_executable(ContraRecompExperiment
    ${NESRECOMP_RUNNER_SOURCES}
    extras.c
    generated/contra_full.c
    generated/contra_dispatch.c
)

target_include_directories(ContraRecompExperiment PRIVATE
    ${NESRECOMP_RUNNER_INCLUDE_DIRS}
    ${CMAKE_SOURCE_DIR}
)

target_compile_definitions(ContraRecompExperiment PRIVATE SDL_MAIN_HANDLED)
target_link_libraries(ContraRecompExperiment PRIVATE SDL2::SDL2)

if(WIN32)
    target_link_libraries(ContraRecompExperiment PRIVATE ws2_32)
endif()
