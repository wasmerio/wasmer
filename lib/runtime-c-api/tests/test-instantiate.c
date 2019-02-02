#include <stdio.h>
#include "../wasmer.h"

int main()
{
    wasmer_import_object_t *import_object = wasmer_import_object_new();
    wasmer_import_object_destroy(import_object);
    return 0;
}