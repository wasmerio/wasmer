cmake_minimum_required (VERSION 2.6)
project (WasmerCApiTests)

add_executable(test-instantiate test-instantiate.c)
add_executable(test-import-function test-import-function.c)

find_library(
        WASMER_LIB NAMES libwasmer_runtime_c_api.dylib libwasmer_runtime_c_api.so libwasmer_runtime_c_api.dll
        PATHS ${CMAKE_SOURCE_DIR}/../../../target/debug/
)

if(NOT WASMER_LIB)
    message(FATAL_ERROR "wasmer library not found")
endif()

target_link_libraries(test-instantiate
                general ${WASMER_LIB})
target_link_libraries(test-import-function
                general ${WASMER_LIB})

enable_testing()
add_test(test-instantiate test-instantiate)
add_test(test-import-function test-import-function)

