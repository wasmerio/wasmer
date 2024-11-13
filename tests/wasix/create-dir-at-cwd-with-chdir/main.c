#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

// the difference between this test and the one in create-dir-at-cwd is the
// presence of chdir.

// this will force chdir to be linked with this binary which in turn will change 
// the behavior of rel_path logic the libc. 
//
// for more info see: libc-find-relpath.h in wasix-libc
int (*dummy_chdir_ref)(const char *path) = chdir;

int main() {
    int status = EXIT_FAILURE;

    const char *dirName1 = "test1";
    if (mkdir(dirName1, 0755) != 0) {
        goto end;
    }

    const char *dirName2 = "./test2";
    if (mkdir(dirName2, 0755) != 0) {
        goto end;
    }

    status = EXIT_SUCCESS;

end:
    printf("%d", status);
    return 0;
}
