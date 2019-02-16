cmake_minimum_required (VERSION 2.6)
project (WasmerCApiTests)

add_executable(test-exports test-exports.c)
add_executable(test-globals test-globals.c)
add_executable(test-instantiate test-instantiate.c)
add_executable(test-import-function test-import-function.c)
add_executable(test-memory test-memory.c)
add_executable(test-module test-module.c)
add_executable(test-validate test-validate.c)
add_executable(test-tables test-tables.c)

find_library(
        WASMER_LIB NAMES libwasmer_runtime_c_api.dylib libwasmer_runtime_c_api.so libwasmer_runtime_c_api.dll
        PATHS ${CMAKE_SOURCE_DIR}/../../../target/debug/
)

if(NOT WASMER_LIB)
    message(FATAL_ERROR "wasmer library not found")
endif()

target_link_libraries(test-exports
        general ${WASMER_LIB})
target_link_libraries(test-globals
        general ${WASMER_LIB})
target_link_libraries(test-instantiate
                general ${WASMER_LIB})
target_link_libraries(test-import-function
                general ${WASMER_LIB})
target_link_libraries(test-memory
        general ${WASMER_LIB})
target_link_libraries(test-module
        general ${WASMER_LIB})
target_link_libraries(test-validate
        general ${WASMER_LIB})
target_link_libraries(test-tables
        general ${WASMER_LIB})

enable_testing()
add_test(test-exports test-exports)
add_test(test-globals test-globals)
add_test(test-instantiate test-instantiate)
add_test(test-import-function test-import-function)
add_test(test-memory test-memory)
add_test(test-module test-module)
add_test(test-validate test-validate)
add_test(test-tables test-tables)

