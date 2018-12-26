#include <stdio.h>
#include <stdlib.h>
int main()
{   
    printf("INIT\n");
    const char* UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("UNEXISTENT_ENVVAR = %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("Setting UNEXISTENT_ENVVAR=PUTENV (via putenv)\n");
    putenv("UNEXISTENT_ENVVAR=PUTENV");
    UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("UNEXISTENT_ENVVAR = %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("Setting UNEXISTENT_ENVVAR=SETENV (via setenv, overwrite)\n");
    setenv("UNEXISTENT_ENVVAR", "SETENV", 1);
    UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("UNEXISTENT_ENVVAR = %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("Setting UNEXISTENT_ENVVAR=SETENV_NEW (via setenv, NO overwrite)\n");
    setenv("UNEXISTENT_ENVVAR", "SETENV_NEW", 0);
    UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("UNEXISTENT_ENVVAR = %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("Unsetting UNEXISTENT_ENVVAR\n");
    unsetenv("UNEXISTENT_ENVVAR");
    UNEXISTENT_ENVVAR = getenv("UNEXISTENT_ENVVAR");
    printf("UNEXISTENT_ENVVAR = %s\n",(UNEXISTENT_ENVVAR!=NULL)? UNEXISTENT_ENVVAR : "[NULL]");
    printf("END\n");
}
