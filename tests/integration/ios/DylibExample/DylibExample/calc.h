//
//  WASM.hpp
//  DylibExample
//
//  Created by Nathan Horrigan on 17/08/2021.
//

#ifndef calc_h
#define calc_h
#include "wasm.h"
#include <stdio.h>

#ifdef __cplusplus
extern "C"
{
#endif

    int calculate_sum(int a, int b);

#ifdef __cplusplus
}
#endif

#endif /* calc_h */
