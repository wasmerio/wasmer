set(NAPI_V8_PREBUILT_VERSION "11.9.2")
set(NAPI_V8_PREBUILT_BASE_URL
  "https://github.com/wasmerio/v8-custom-builds/releases/download/${NAPI_V8_PREBUILT_VERSION}")

function(_napi_v8_warn_deprecated old_name new_name)
  message(WARNING
    "${old_name} is deprecated; use ${new_name} instead.")
endfunction()

function(_napi_v8_read_value out_var canonical_name)
  set(_resolved "")

  if(DEFINED ${canonical_name} AND NOT "${${canonical_name}}" STREQUAL "")
    set(_resolved "${${canonical_name}}")
    set(${out_var} "${_resolved}" PARENT_SCOPE)
    return()
  endif()

  if(DEFINED ENV{${canonical_name}} AND NOT "$ENV{${canonical_name}}" STREQUAL "")
    set(_resolved "$ENV{${canonical_name}}")
    set(${out_var} "${_resolved}" PARENT_SCOPE)
    return()
  endif()

  foreach(_old_name IN LISTS ARGN)
    if(DEFINED ${_old_name} AND NOT "${${_old_name}}" STREQUAL "")
      _napi_v8_warn_deprecated("${_old_name}" "${canonical_name}")
      set(_resolved "${${_old_name}}")
      set(${out_var} "${_resolved}" PARENT_SCOPE)
      return()
    endif()
    if(DEFINED ENV{${_old_name}} AND NOT "$ENV{${_old_name}}" STREQUAL "")
      _napi_v8_warn_deprecated("${_old_name}" "${canonical_name}")
      set(_resolved "$ENV{${_old_name}}")
      set(${out_var} "${_resolved}" PARENT_SCOPE)
      return()
    endif()
  endforeach()

  set(${out_var} "" PARENT_SCOPE)
endfunction()

function(_napi_v8_local_strategy out_ok out_include out_library out_extra out_defines out_reason
    include_override library_override extra_override defines_override)
  set(_include "${include_override}")
  set(_library "${library_override}")
  set(_extra "${extra_override}")
  set(_defines "${defines_override}")
  set(_reason "")

  if(APPLE)
    foreach(_hb_prefix "/opt/homebrew/Cellar/v8/14.5.201.9" "/opt/homebrew/opt/v8")
      if(NOT _include AND EXISTS "${_hb_prefix}/include")
        set(_include "${_hb_prefix}/include")
      endif()
      if(NOT _library AND EXISTS "${_hb_prefix}/lib/libv8.dylib")
        set(_library "${_hb_prefix}/lib/libv8.dylib")
      endif()
      if(_library STREQUAL "${_hb_prefix}/lib/libv8.dylib")
        if(_extra STREQUAL "")
          set(_extra "${_hb_prefix}/lib/libv8_libplatform.dylib;${_hb_prefix}/lib/libv8_libbase.dylib")
        endif()
        if(_defines STREQUAL "")
          set(_defines "")
        endif()
        break()
      endif()
    endforeach()
  endif()

  if(_include STREQUAL "")
    set(_reason "V8 include directory is not set. Set NAPI_V8_INCLUDE_DIR.")
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason} "${_reason}" PARENT_SCOPE)
    return()
  endif()
  if(NOT EXISTS "${_include}")
    set(_reason "V8 include directory does not exist: ${_include}")
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason} "${_reason}" PARENT_SCOPE)
    return()
  endif()

  if(_library STREQUAL "")
    set(_reason "V8 library path is not set. Set NAPI_V8_LIBRARY.")
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason} "${_reason}" PARENT_SCOPE)
    return()
  endif()
  if(NOT EXISTS "${_library}")
    set(_reason "V8 library does not exist: ${_library}")
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason} "${_reason}" PARENT_SCOPE)
    return()
  endif()

  set(${out_ok} TRUE PARENT_SCOPE)
  set(${out_include} "${_include}" PARENT_SCOPE)
  set(${out_library} "${_library}" PARENT_SCOPE)
  set(${out_extra} "${_extra}" PARENT_SCOPE)
  set(${out_defines} "${_defines}" PARENT_SCOPE)
  set(${out_reason} "" PARENT_SCOPE)
endfunction()

function(_napi_v8_prebuilt_strategy out_ok out_include out_library out_extra out_defines out_reason
    extra_override defines_override)
  set(_platform "")
  set(_asset "")
  if(CMAKE_SYSTEM_NAME STREQUAL "Darwin" AND
     (CMAKE_SYSTEM_PROCESSOR STREQUAL "arm64" OR CMAKE_SYSTEM_PROCESSOR STREQUAL "aarch64"))
    set(_platform "darwin-arm64")
    set(_asset "v8-darwin-arm64.tar.xz")
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux" AND
         (CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64" OR CMAKE_SYSTEM_PROCESSOR STREQUAL "amd64"))
    set(_platform "linux-amd64")
    set(_asset "v8-linux-amd64.tar.xz")
  else()
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason}
      "Prebuilt V8 is not available for ${CMAKE_SYSTEM_NAME}/${CMAKE_SYSTEM_PROCESSOR}"
      PARENT_SCOPE)
    return()
  endif()

  set(_cache_dir "${CMAKE_BINARY_DIR}/_v8_cache/${NAPI_V8_PREBUILT_VERSION}/${_platform}")
  set(_archive_path "${_cache_dir}/${_asset}")
  set(_include "${_cache_dir}/include")
  set(_library "${_cache_dir}/lib/libv8.a")

  file(MAKE_DIRECTORY "${_cache_dir}")

  if(NOT EXISTS "${_archive_path}")
    set(_url "${NAPI_V8_PREBUILT_BASE_URL}/${_asset}")
    message(STATUS "Downloading prebuilt V8 ${NAPI_V8_PREBUILT_VERSION} from ${_url}")
    file(DOWNLOAD
      "${_url}"
      "${_archive_path}"
      STATUS _download_status
      SHOW_PROGRESS
      TLS_VERIFY ON
    )
    list(GET _download_status 0 _download_code)
    list(GET _download_status 1 _download_message)
    if(NOT _download_code EQUAL 0)
      set(${out_ok} FALSE PARENT_SCOPE)
      set(${out_reason}
        "Failed to download ${_url}: ${_download_message}"
        PARENT_SCOPE)
      return()
    endif()
  endif()

  if(NOT EXISTS "${_include}" OR NOT EXISTS "${_library}")
    execute_process(
      COMMAND "${CMAKE_COMMAND}" -E tar xJf "${_archive_path}"
      WORKING_DIRECTORY "${_cache_dir}"
      RESULT_VARIABLE _extract_result
      ERROR_VARIABLE _extract_error
    )
    if(NOT _extract_result EQUAL 0)
      set(${out_ok} FALSE PARENT_SCOPE)
      set(${out_reason}
        "Failed to extract ${_archive_path}: ${_extract_error}"
        PARENT_SCOPE)
      return()
    endif()
  endif()

  if(NOT EXISTS "${_include}" OR NOT EXISTS "${_library}")
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason}
      "Prebuilt V8 archive did not produce expected layout under ${_cache_dir}"
      PARENT_SCOPE)
    return()
  endif()

  if(defines_override STREQUAL "")
    set(_defines "V8_COMPRESS_POINTERS")
  else()
    set(_defines "${defines_override}")
  endif()

  if(extra_override STREQUAL "" AND CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(_extra "/System/Library/Frameworks/CoreFoundation.framework")
  else()
    set(_extra "${extra_override}")
  endif()

  set(${out_ok} TRUE PARENT_SCOPE)
  set(${out_include} "${_include}" PARENT_SCOPE)
  set(${out_library} "${_library}" PARENT_SCOPE)
  set(${out_extra} "${_extra}" PARENT_SCOPE)
  set(${out_defines} "${_defines}" PARENT_SCOPE)
  set(${out_reason} "" PARENT_SCOPE)
endfunction()

function(_napi_v8_source_strategy out_ok out_include out_library out_extra out_defines out_reason
    extra_override defines_override)
  set(_v8_source_root "${PROJECT_ROOT}/deps/v8")
  if(NOT EXISTS "${_v8_source_root}/BUILD.gn")
    set(${out_ok} FALSE PARENT_SCOPE)
    set(${out_reason} "Could not find V8 source tree at ${_v8_source_root}" PARENT_SCOPE)
    return()
  endif()

  find_program(_python3 python3)
  find_program(_gn gn)
  find_program(_autoninja autoninja)
  find_program(_ninja ninja)
  if(NOT _python3)
    message(FATAL_ERROR "NAPI_V8_BUILD_METHOD=source requires python3 in PATH.")
  endif()
  if(NOT _gn)
    message(FATAL_ERROR "NAPI_V8_BUILD_METHOD=source requires gn in PATH.")
  endif()
  if(NOT _autoninja AND NOT _ninja)
    message(FATAL_ERROR "NAPI_V8_BUILD_METHOD=source requires ninja or autoninja in PATH.")
  endif()

  if(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(_target_os "mac")
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(_target_os "linux")
  else()
    message(FATAL_ERROR "NAPI_V8_BUILD_METHOD=source is unsupported for ${CMAKE_SYSTEM_NAME}.")
  endif()

  if(CMAKE_SYSTEM_PROCESSOR STREQUAL "arm64" OR CMAKE_SYSTEM_PROCESSOR STREQUAL "aarch64")
    set(_target_cpu "arm64")
  elseif(CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64" OR CMAKE_SYSTEM_PROCESSOR STREQUAL "amd64")
    set(_target_cpu "x64")
  else()
    message(FATAL_ERROR
      "NAPI_V8_BUILD_METHOD=source unsupported architecture: ${CMAKE_SYSTEM_PROCESSOR}")
  endif()

  set(_platform "${_target_os}-${_target_cpu}")
  set(_build_dir "${CMAKE_BINARY_DIR}/_v8_source_build/${_platform}")
  file(MAKE_DIRECTORY "${_build_dir}")

  set(_gn_args
    "is_debug=false "
    "is_component_build=false "
    "v8_monolithic=true "
    "v8_use_external_startup_data=false "
    "v8_enable_i18n_support=true "
    "icu_use_data_file=false "
    "v8_enable_pointer_compression=true "
    "v8_enable_sandbox=false "
    "target_os=\"${_target_os}\" "
    "target_cpu=\"${_target_cpu}\" "
    "treat_warnings_as_errors=false")

  message(STATUS "Generating V8 build files in ${_build_dir}")
  execute_process(
    COMMAND "${_gn}" gen "${_build_dir}" "--args=${_gn_args}"
    WORKING_DIRECTORY "${_v8_source_root}"
    RESULT_VARIABLE _gn_result
    ERROR_VARIABLE _gn_error
  )
  if(NOT _gn_result EQUAL 0)
    message(FATAL_ERROR "gn gen failed: ${_gn_error}")
  endif()

  if(_autoninja)
    set(_ninja_cmd "${_autoninja}")
  else()
    set(_ninja_cmd "${_ninja}")
  endif()
  message(STATUS "Building V8 monolith in ${_build_dir}")
  execute_process(
    COMMAND "${_ninja_cmd}" -C "${_build_dir}" v8_monolith
    WORKING_DIRECTORY "${_v8_source_root}"
    RESULT_VARIABLE _ninja_result
    ERROR_VARIABLE _ninja_error
  )
  if(NOT _ninja_result EQUAL 0)
    message(FATAL_ERROR "V8 source build failed: ${_ninja_error}")
  endif()

  set(_library_candidates
    "${_build_dir}/obj/libv8_monolith.a"
    "${_build_dir}/obj/libv8_monolith/libv8_monolith.a")
  set(_library "")
  foreach(_candidate IN LISTS _library_candidates)
    if(EXISTS "${_candidate}")
      set(_library "${_candidate}")
      break()
    endif()
  endforeach()
  if(_library STREQUAL "")
    message(FATAL_ERROR
      "V8 source build succeeded but libv8_monolith.a was not found in ${_build_dir}/obj.")
  endif()

  set(_include "${_v8_source_root}/include")
  if(NOT EXISTS "${_include}")
    message(FATAL_ERROR "V8 include directory missing after source build: ${_include}")
  endif()

  if(defines_override STREQUAL "")
    set(_defines "V8_COMPRESS_POINTERS")
  else()
    set(_defines "${defines_override}")
  endif()

  if(extra_override STREQUAL "" AND CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(_extra "/System/Library/Frameworks/CoreFoundation.framework")
  else()
    set(_extra "${extra_override}")
  endif()

  set(${out_ok} TRUE PARENT_SCOPE)
  set(${out_include} "${_include}" PARENT_SCOPE)
  set(${out_library} "${_library}" PARENT_SCOPE)
  set(${out_extra} "${_extra}" PARENT_SCOPE)
  set(${out_defines} "${_defines}" PARENT_SCOPE)
  set(${out_reason} "" PARENT_SCOPE)
endfunction()

function(napi_v8_resolve_configuration)
  set(_requested_method "prebuilt")
  if(DEFINED NAPI_V8_BUILD_METHOD AND NOT "${NAPI_V8_BUILD_METHOD}" STREQUAL "")
    set(_requested_method "${NAPI_V8_BUILD_METHOD}")
  elseif(DEFINED ENV{NAPI_V8_BUILD_METHOD} AND NOT "$ENV{NAPI_V8_BUILD_METHOD}" STREQUAL "")
    set(_requested_method "$ENV{NAPI_V8_BUILD_METHOD}")
  endif()
  string(TOLOWER "${_requested_method}" _requested_method)

  if(DEFINED ENV{NAPI_V8_FORCE_LOCAL_BUILD} AND
     NOT "$ENV{NAPI_V8_FORCE_LOCAL_BUILD}" STREQUAL "" AND
     NOT "$ENV{NAPI_V8_FORCE_LOCAL_BUILD}" STREQUAL "0")
    set(_requested_method "local")
  endif()

  if(NOT _requested_method MATCHES "^(prebuilt|source|local)$")
    message(FATAL_ERROR
      "Invalid NAPI_V8_BUILD_METHOD='${_requested_method}'. "
      "Valid values: prebuilt, source, local.")
  endif()

  _napi_v8_read_value(_include_override "NAPI_V8_INCLUDE_DIR" "NAPI_V8_V8_INCLUDE_DIR")
  _napi_v8_read_value(_library_override "NAPI_V8_LIBRARY" "NAPI_V8_V8_LIBRARY" "NAPI_V8_V8_MONOLITH_LIB")
  _napi_v8_read_value(_extra_override "NAPI_V8_EXTRA_LIBS" "NAPI_V8_V8_EXTRA_LIBS")
  _napi_v8_read_value(_defines_override "NAPI_V8_DEFINES" "NAPI_V8_V8_DEFINES")

  set(_resolved_ok FALSE)
  set(_resolved_include "")
  set(_resolved_library "")
  set(_resolved_extra "")
  set(_resolved_defines "")
  set(_resolved_reason "")
  set(_effective_method "${_requested_method}")

  if(NOT "${_include_override}" STREQUAL "" OR NOT "${_library_override}" STREQUAL "")
    _napi_v8_local_strategy(
      _resolved_ok _resolved_include _resolved_library _resolved_extra _resolved_defines _resolved_reason
      "${_include_override}" "${_library_override}" "${_extra_override}" "${_defines_override}")
    set(_effective_method "local")
    if(NOT _resolved_ok)
      message(FATAL_ERROR "${_resolved_reason}")
    endif()
  elseif(_requested_method STREQUAL "prebuilt")
    _napi_v8_prebuilt_strategy(
      _resolved_ok _resolved_include _resolved_library _resolved_extra _resolved_defines _resolved_reason
      "${_extra_override}" "${_defines_override}")
    if(NOT _resolved_ok)
      message(WARNING
        "Prebuilt V8 unavailable: ${_resolved_reason}. Falling back to local mode.")
      _napi_v8_local_strategy(
        _resolved_ok _resolved_include _resolved_library _resolved_extra _resolved_defines _resolved_reason
        "${_include_override}" "${_library_override}" "${_extra_override}" "${_defines_override}")
      set(_effective_method "local")
      if(NOT _resolved_ok)
        message(FATAL_ERROR "${_resolved_reason}")
      endif()
    endif()
  elseif(_requested_method STREQUAL "source")
    _napi_v8_source_strategy(
      _resolved_ok _resolved_include _resolved_library _resolved_extra _resolved_defines _resolved_reason
      "${_extra_override}" "${_defines_override}")
  elseif(_requested_method STREQUAL "local")
    _napi_v8_local_strategy(
      _resolved_ok _resolved_include _resolved_library _resolved_extra _resolved_defines _resolved_reason
      "${_include_override}" "${_library_override}" "${_extra_override}" "${_defines_override}")
    if(NOT _resolved_ok)
      message(FATAL_ERROR "${_resolved_reason}")
    endif()
  endif()

  set(NAPI_V8_BUILD_METHOD "${_requested_method}" CACHE STRING
    "V8 resolution method: prebuilt|source|local" FORCE)
  set_property(CACHE NAPI_V8_BUILD_METHOD PROPERTY STRINGS prebuilt source local)

  set(NAPI_V8_EFFECTIVE_BUILD_METHOD "${_effective_method}" CACHE STRING
    "Effective resolved V8 method" FORCE)
  set(NAPI_V8_INCLUDE_DIR "${_resolved_include}" CACHE PATH "Path to V8 headers" FORCE)
  set(NAPI_V8_LIBRARY "${_resolved_library}" CACHE FILEPATH
    "Path to V8 runtime library (monolith/static/dynamic)" FORCE)
  set(NAPI_V8_EXTRA_LIBS "${_resolved_extra}" CACHE STRING
    "Semicolon-separated extra V8 libraries (e.g. libplatform/libbase)" FORCE)
  set(NAPI_V8_DEFINES "${_resolved_defines}" CACHE STRING
    "Semicolon-separated V8 embedder compile definitions" FORCE)

  # Compatibility aliases for existing callers.
  set(NAPI_V8_V8_INCLUDE_DIR "${NAPI_V8_INCLUDE_DIR}" CACHE PATH "Deprecated alias for NAPI_V8_INCLUDE_DIR" FORCE)
  set(NAPI_V8_V8_LIBRARY "${NAPI_V8_LIBRARY}" CACHE FILEPATH "Deprecated alias for NAPI_V8_LIBRARY" FORCE)
  set(NAPI_V8_V8_MONOLITH_LIB "${NAPI_V8_LIBRARY}" CACHE FILEPATH "Deprecated alias for NAPI_V8_LIBRARY" FORCE)
  set(NAPI_V8_V8_EXTRA_LIBS "${NAPI_V8_EXTRA_LIBS}" CACHE STRING "Deprecated alias for NAPI_V8_EXTRA_LIBS" FORCE)
  set(NAPI_V8_V8_DEFINES "${NAPI_V8_DEFINES}" CACHE STRING "Deprecated alias for NAPI_V8_DEFINES" FORCE)

  message(STATUS "Resolved V8 method: ${NAPI_V8_EFFECTIVE_BUILD_METHOD}")
  message(STATUS "Resolved V8 include dir: ${NAPI_V8_INCLUDE_DIR}")
  message(STATUS "Resolved V8 library: ${NAPI_V8_LIBRARY}")
endfunction()
