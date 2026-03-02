#include <stdio.h>
#ifdef STATIC_THROWER
#include "thrower.cpp"
#else
#include "thrower.hpp"
#endif
#ifdef STATIC_CATCHER
#include "catcher.cpp"
#else
#include "catcher.hpp"
#endif

int main() {
    catch_exception();
    return 0;
}