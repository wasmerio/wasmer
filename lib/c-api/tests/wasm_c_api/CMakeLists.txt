cmake_minimum_required (VERSION 2.6)
project (WasmerWasmCApiTests)

# Examples as tests from the `wasm-c-api` repository.
add_executable(wasm-c-api-callback wasm-c-api/example/callback.c)
#add_executable(wasm-c-api-finalize wasm-c-api/example/finalize.c)
#add_executable(wasm-c-api-global wasm-c-api/example/global.c)
add_executable(wasm-c-api-hello wasm-c-api/example/hello.c)
#add_executable(wasm-c-api-hostref wasm-c-api/example/hostref.c)
#add_executable(wasm-c-api-memory wasm-c-api/example/memory.c)
#add_executable(wasm-c-api-multi wasm-c-api/example/multi.c)
add_executable(wasm-c-api-reflect wasm-c-api/example/reflect.c)
add_executable(wasm-c-api-serialize wasm-c-api/example/serialize.c)
#add_executable(wasm-c-api-start wasm-c-api/example/start.c)
#add_executable(wasm-c-api-table wasm-c-api/example/table.c)
#add_executable(wasm-c-api-threads wasm-c-api/example/threads.c)
add_executable(wasm-c-api-trap wasm-c-api/example/trap.c)

# Our additional tests.
add_executable(test-wasi test-wasi.c)
#add_executable(test-early-exit test-early-exit.c)

include_directories(wasm-c-api/include)
include_directories(../../)

find_library(
    WASMER_LIB NAMES libwasmer_c_api.dylib libwasmer_c_api.so wasmer_c_api.dll
    PATHS ${CMAKE_SOURCE_DIR}/../../../../target/release/
)

if(NOT WASMER_LIB)
    message(FATAL_ERROR "wasmer library not found")
endif()

enable_testing()

set(
    COMPILER_OPTIONS
    # Clang or gcc
    $<$<OR:$<CXX_COMPILER_ID:Clang>,$<CXX_COMPILER_ID:GNU>>:
        "-Werror" >
    # MSVC
    $<$<CXX_COMPILER_ID:MSVC>:
        "/WX" >
)

target_link_libraries(wasm-c-api-callback general ${WASMER_LIB})
target_compile_options(wasm-c-api-callback PRIVATE ${COMPILER_OPTIONS})
add_test(NAME wasm-c-api-callback
         COMMAND wasm-c-api-callback
         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
)

#target_link_libraries(wasm-c-api-finalize general ${WASMER_LIB})
#target_compile_options(wasm-c-api-finalize PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-finalize
#         COMMAND wasm-c-api-finalize
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

#target_link_libraries(wasm-c-api-global general ${WASMER_LIB})
#target_compile_options(wasm-c-api-global PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-global
#         COMMAND wasm-c-api-global
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

target_link_libraries(wasm-c-api-hello general ${WASMER_LIB})
target_compile_options(wasm-c-api-hello PRIVATE ${COMPILER_OPTIONS})
add_test(NAME wasm-c-api-hello
         COMMAND wasm-c-api-hello
         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
)

#target_link_libraries(wasm-c-api-hostref general ${WASMER_LIB})
#target_compile_options(wasm-c-api-hostref PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-hostref
#         COMMAND wasm-c-api-hostref
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

#target_link_libraries(wasm-c-api-memory general ${WASMER_LIB})
#target_compile_options(wasm-c-api-memory PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-memory
#         COMMAND wasm-c-api-memory
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

#target_link_libraries(wasm-c-api-multi general ${WASMER_LIB})
#target_compile_options(wasm-c-api-multi PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-multi
#         COMMAND wasm-c-api-multi
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

target_link_libraries(wasm-c-api-reflect general ${WASMER_LIB})
target_compile_options(wasm-c-api-reflect PRIVATE ${COMPILER_OPTIONS})
add_test(NAME wasm-c-api-reflect
         COMMAND wasm-c-api-reflect
         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
)

target_link_libraries(wasm-c-api-serialize general ${WASMER_LIB})
target_compile_options(wasm-c-api-serialize PRIVATE ${COMPILER_OPTIONS})
add_test(NAME wasm-c-api-serialize
         COMMAND wasm-c-api-serialize
         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
)

#target_link_libraries(wasm-c-api-start general ${WASMER_LIB})
#target_compile_options(wasm-c-api-start PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-start
#         COMMAND wasm-c-api-start
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

#target_link_libraries(wasm-c-api-table general ${WASMER_LIB})
#target_compile_options(wasm-c-api-table PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-table
#         COMMAND wasm-c-api-table
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

#target_link_libraries(wasm-c-api-threads general ${WASMER_LIB})
#target_compile_options(wasm-c-api-threads PRIVATE ${COMPILER_OPTIONS})
#add_test(NAME wasm-c-api-threads
#         COMMAND wasm-c-api-threads
#         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
#)

target_link_libraries(wasm-c-api-trap general ${WASMER_LIB})
target_compile_options(wasm-c-api-trap PRIVATE ${COMPILER_OPTIONS})
add_test(NAME wasm-c-api-trap
         COMMAND wasm-c-api-trap
         WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/wasm-c-api/example/
)

set_property(TARGET test-wasi PROPERTY C_STANDARD 11)
target_link_libraries(test-wasi general ${WASMER_LIB})
target_compile_options(test-wasi PRIVATE ${COMPILER_OPTIONS})
add_test(test-wasi test-wasi)

#set_property(TARGET test-early-exit PROPERTY C_STANDARD 11)
#target_link_libraries(test-early-exit general ${WASMER_LIB})
#target_compile_options(test-early-exit PRIVATE ${COMPILER_OPTIONS})
#add_test(test-early-exit test-early-exit)
