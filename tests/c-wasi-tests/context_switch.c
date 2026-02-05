#include <assert.h>
#include <errno.h>
#include <stdio.h>

#include <wasix/context.h>

static wasix_context_id_t ctx1;
static int phase = 0;

static void context1_fn(void)
{
    int ret;

    // Cannot destroy the active context (self).
    ret = wasix_context_destroy(ctx1);
    assert(ret == -1);
    assert(errno == EINVAL);

    phase = 1;
    wasix_context_switch(wasix_context_main);

    // If resumed again, just yield back to main.
    wasix_context_switch(wasix_context_main);
}

int main(void)
{
    printf("WASIX context_switch integration tests\n");

    int ret = wasix_context_create(&ctx1, context1_fn);
    assert(ret == 0);

    printf("Test 1: switch to main (no-op)\n");
    ret = wasix_context_switch(wasix_context_main);
    assert(ret == 0);

    printf("Test 2: switch to new context and back\n");
    ret = wasix_context_switch(ctx1);
    assert(ret == 0);
    assert(phase == 1);

    printf("Test 3: destroy main context fails\n");
    ret = wasix_context_destroy(wasix_context_main);
    assert(ret == -1);
    assert(errno == EINVAL);

    printf("Test 4: destroy context succeeds\n");
    ret = wasix_context_destroy(ctx1);
    assert(ret == 0);

    printf("Test 5: switching to destroyed context fails\n");
    errno = 0;
    ret = wasix_context_switch(ctx1);
    assert(ret == -1);
    assert(errno == EINVAL);

    printf("Test 6: destroy already destroyed context is no-op\n");
    ret = wasix_context_destroy(ctx1);
    assert(ret == 0);

    printf("All tests passed!\n");
    return 0;
}
