function(napi_engine_set_layout ENGINE_ROOT)
  get_filename_component(_napi_root "${ENGINE_ROOT}" DIRECTORY)
  get_filename_component(_project_root "${_napi_root}" DIRECTORY)

  set(NAPI_ROOT "${_napi_root}" PARENT_SCOPE)
  set(PROJECT_ROOT "${_project_root}" PARENT_SCOPE)
  set(NAPI_INCLUDE_ROOT "${_napi_root}/include" PARENT_SCOPE)
  set(NAPI_TESTS_ROOT "${_napi_root}/tests" PARENT_SCOPE)
endfunction()
