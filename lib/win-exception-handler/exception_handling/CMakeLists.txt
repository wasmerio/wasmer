cmake_minimum_required(VERSION 3.0)
project(exception_handling C)

add_library(exception_handling STATIC exception_handling.c)

install(TARGETS exception_handling DESTINATION .)
