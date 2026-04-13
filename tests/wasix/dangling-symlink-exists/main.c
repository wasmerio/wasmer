/*
 * Regression test for cached dangling symlink targets.
 *
 * When a symlink's target is deleted, os.path.exists() (stat with follow_symlinks)
 * must return false.  The bug was that get_inode_at_path_inner returned the cached
 * symlink inode directly instead of continuing symlink resolution, so stat()
 * succeeded even for a dangling symlink.
 *
 */

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static const char *TARGET = "tmp_target";
static const char *LINK   = "tmp_link";

static int do_islink(const char *path) {
    struct stat st;
    if (lstat(path, &st) != 0) return 0;
    return S_ISLNK(st.st_mode) ? 1 : 0;
}

static int do_exists(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? 1 : 0;
}

static int do_lexists(const char *path) {
    struct stat st;
    return lstat(path, &st) == 0 ? 1 : 0;
}

int main(void) {
    /* Clean up any leftover state from a previous run. */
    unlink(LINK);
    unlink(TARGET);

    /* Create target file. */
    int fd = open(TARGET, O_CREAT | O_WRONLY | O_TRUNC, 0666);
    if (fd < 0) { perror("open target"); return 1; }
    if (write(fd, "foo", 3) != 3) { perror("write target"); return 1; }
    if (close(fd) != 0) { perror("close target"); return 1; }

    /* Create symlink. */
    if (symlink(TARGET, LINK) != 0) { perror("symlink"); return 1; }

    /*
     * Warm up the inode cache by resolving the link once.
     * This is the step that populates the cache entry that the bug left stale.
     */
    int islink_before  = do_islink(LINK);
    int exists_before  = do_exists(LINK);
    int lexists_before = do_lexists(LINK);

    /* Remove the target, making the symlink dangling. */
    if (unlink(TARGET) != 0) { perror("unlink target"); return 1; }

    /* Re-query — now the symlink is dangling. */
    int islink_after  = do_islink(LINK);
    int exists_after  = do_exists(LINK);
    int lexists_after = do_lexists(LINK);

    /* Cleanup. */
    unlink(LINK);

    /* Verify before-removal results. */
    if (!islink_before) {
        fprintf(stderr, "before: expected islink=1, got 0\n"); return 1;
    }
    if (!exists_before) {
        fprintf(stderr, "before: expected exists=1, got 0\n"); return 1;
    }
    if (!lexists_before) {
        fprintf(stderr, "before: expected lexists=1, got 0\n"); return 1;
    }

    /* Verify after-removal results. */
    if (!islink_after) {
        fprintf(stderr, "after: expected islink=1, got 0\n"); return 1;
    }
    if (exists_after) {
        fprintf(stderr,
                "after: expected exists=0 for dangling symlink, got 1 "
                "(cached inode bug)\n");
        return 1;
    }
    if (!lexists_after) {
        fprintf(stderr, "after: expected lexists=1, got 0\n"); return 1;
    }

    printf("0");
    return 0;
}
