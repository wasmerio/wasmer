cmake_minimum_required(VERSION 3.20)
project(napi_engines LANGUAGES C CXX)

option(NAPI_BUILD_V8 "Build the V8-backed Node-API engine" ON)

if(NAPI_BUILD_V8)
  add_subdirectory(v8)
endif()
