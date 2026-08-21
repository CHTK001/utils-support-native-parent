cmake_minimum_required(VERSION 3.16)
project(shmqueue C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_C_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

# MSVC 需要显式 /std:c11 才支持 <stdatomic.h>
if(MSVC)
    add_compile_options(/std:c11)
endif()

option(SHMQUEUE_BUILD_TESTS "构建 C 单元测试" ON)
option(SHMQUEUE_BUILD_SHARED "构建为动态库" ON)

set(SHMQUEUE_SRC_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/main/c")
set(SHMQUEUE_SOURCES ${SHMQUEUE_SRC_DIR}/src/shmqueue.c ${SHMQUEUE_SRC_DIR}/src/shmchan.c)
set(SHMQUEUE_HEADERS ${SHMQUEUE_SRC_DIR}/include/shmqueue.h ${SHMQUEUE_SRC_DIR}/include/shmchan.h ${SHMQUEUE_SRC_DIR}/src/atomic_ops.h)

if(SHMQUEUE_BUILD_SHARED)
    add_library(shmqueue SHARED ${SHMQUEUE_SOURCES} ${SHMQUEUE_HEADERS})
    set_target_properties(shmqueue PROPERTIES
        VERSION   1.0.0
        SOVERSION 1
        PUBLIC_HEADER "${SHMQUEUE_SRC_DIR}/include/shmqueue.h"
    )
    # Windows MSVC 默认不导出符号
    set_target_properties(shmqueue PROPERTIES WINDOWS_EXPORT_ALL_SYMBOLS ON)
else()
    add_library(shmqueue STATIC ${SHMQUEUE_SOURCES} ${SHMQUEUE_HEADERS})
endif()

target_include_directories(shmqueue PUBLIC
    $<BUILD_INTERFACE:${SHMQUEUE_SRC_DIR}/include>
    $<BUILD_INTERFACE:${SHMQUEUE_SRC_DIR}/src>
)

if(MSVC)
    target_compile_options(shmqueue PRIVATE /W3)
    target_compile_definitions(shmqueue PRIVATE SHMQUEUE_BUILDING_DLL)
else()
    target_compile_options(shmqueue PRIVATE
        -Wall -Wextra -Wpedantic -Wno-unused-parameter
        -fvisibility=hidden
    )
endif()

if(UNIX AND NOT APPLE)
    target_link_libraries(shmqueue PRIVATE rt)
endif()

# 输出到 Maven resources/native
if(NOT SHMQUEUE_RESOURCE_DIR)
    set(SHMQUEUE_RESOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/main/resources/native")
endif()
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(SHMQUEUE_PLATFORM "linux")
elseif(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(SHMQUEUE_PLATFORM "darwin")
elseif(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(SHMQUEUE_PLATFORM "windows")
endif()
if(CMAKE_SYSTEM_PROCESSOR MATCHES "x86_64|AMD64|amd64")
    set(SHMQUEUE_ARCH "x86_64")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64|ARM64")
    set(SHMQUEUE_ARCH "aarch64")
endif()
set(SHMQUEUE_OUTPUT_DIR "${SHMQUEUE_RESOURCE_DIR}/${SHMQUEUE_PLATFORM}-${SHMQUEUE_ARCH}")
add_custom_command(TARGET shmqueue POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SHMQUEUE_OUTPUT_DIR}"
    COMMAND ${CMAKE_COMMAND} -E copy $<TARGET_FILE:shmqueue> "${SHMQUEUE_OUTPUT_DIR}/$<TARGET_FILE_NAME:shmqueue>"
    COMMENT "shmqueue -> ${SHMQUEUE_OUTPUT_DIR}"
)

if(SHMQUEUE_BUILD_TESTS)
    enable_testing()
    add_executable(shmqueue_test
        ${SHMQUEUE_HEADERS}
        ${SHMQUEUE_SRC_DIR}/tests/test_shmqueue.c
    )
    target_include_directories(shmqueue_test PRIVATE
        ${SHMQUEUE_SRC_DIR}/include
        ${SHMQUEUE_SRC_DIR}/src
    )
    target_link_libraries(shmqueue_test PRIVATE shmqueue)
    if(UNIX AND NOT APPLE)
        target_link_libraries(shmqueue_test PRIVATE rt pthread)
    endif()
    add_test(NAME shmqueue_basic COMMAND shmqueue_test)
endif()

option(SHMQUEUE_BUILD_BENCH "构建吞吐基准" ON)
if(SHMQUEUE_BUILD_BENCH)
    add_executable(shmqueue_bench
        ${SHMQUEUE_HEADERS}
        ${SHMQUEUE_SRC_DIR}/bench/bench_shmqueue.c
    )
    target_include_directories(shmqueue_bench PRIVATE
        ${SHMQUEUE_SRC_DIR}/include
        ${SHMQUEUE_SRC_DIR}/src
    )
    target_link_libraries(shmqueue_bench PRIVATE shmqueue)
    if(UNIX AND NOT APPLE)
        target_link_libraries(shmqueue_bench PRIVATE rt pthread)
    endif()
endif()
