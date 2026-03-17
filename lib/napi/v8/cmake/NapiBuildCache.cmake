include_guard(GLOBAL)

option(NAPI_ENABLE_SCCACHE "Use sccache as compiler launcher when available" ON)

function(napi_enable_sccache_if_available)
  if(NOT NAPI_ENABLE_SCCACHE)
    return()
  endif()

  find_program(NAPI_SCCACHE_PROGRAM sccache)
  if(NOT NAPI_SCCACHE_PROGRAM)
    return()
  endif()

  foreach(lang C CXX)
    if(DEFINED CMAKE_${lang}_COMPILER_LAUNCHER AND
       NOT CMAKE_${lang}_COMPILER_LAUNCHER STREQUAL "")
      continue()
    endif()
    set(CMAKE_${lang}_COMPILER_LAUNCHER
        "${NAPI_SCCACHE_PROGRAM}"
        CACHE STRING "Compiler launcher for ${lang}" FORCE)
  endforeach()

  message(STATUS "Using sccache compiler launcher: ${NAPI_SCCACHE_PROGRAM}")
endfunction()

