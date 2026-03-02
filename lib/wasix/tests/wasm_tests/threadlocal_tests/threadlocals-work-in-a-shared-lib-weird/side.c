#include <stdio.h>

int _Thread_local toast = 20;
void increment_toast_from_lib() {
    toast++;
}
void print_toast_from_lib() {
    printf("%d", toast);
}