#include <stdio.h>
#include <stdlib.h>
int main()
{   
    printf("INIT\n");
    const char* UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("get UNEXISTENT_ENVVAR: %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("set UNEXISTENT_ENVVAR = SET\n");
    putenv("UNEXISTENT_ENVVAR=SET");
    UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("get UNEXISTENT_ENVVAR: %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("END\n");
}
