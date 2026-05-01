#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <wasi/api_wasix.h>

static void test_invalid_index(void) {
  __wasi_function_pointer_t invalid_index = 999999;
  __wasi_errno_t ret;

  printf("Test 1: closure_free with unallocated index (idempotent)\n");

  ret = __wasi_closure_free(invalid_index);
  assert(ret == 0);
}

static void test_allocate_and_free(void) {
  __wasi_function_pointer_t closure_index = 0;
  __wasi_errno_t ret_alloc;
  __wasi_errno_t ret_free;

  printf("Test 2: closure_allocate + closure_free (basic lifecycle)\n");

  ret_alloc = __wasi_closure_allocate(&closure_index);
  assert(ret_alloc == 0);
  assert(closure_index != 0);

  ret_free = __wasi_closure_free(closure_index);
  assert(ret_free == 0);
}

static void test_double_free(void) {
  __wasi_function_pointer_t closure_index = 0;
  __wasi_errno_t ret_alloc;
  __wasi_errno_t ret_free1;
  __wasi_errno_t ret_free2;

  printf("Test 3: double-free safety\n");

  ret_alloc = __wasi_closure_allocate(&closure_index);
  assert(ret_alloc == 0);

  ret_free1 = __wasi_closure_free(closure_index);
  assert(ret_free1 == 0);

  ret_free2 = __wasi_closure_free(closure_index);
  assert(ret_free2 == 0);
}

static void test_multiple_cycles(void) {
  int i;

  printf("Test 4: multiple allocate/free cycles\n");

  for (i = 0; i < 10; i++) {
    __wasi_function_pointer_t closure_index = 0;
    __wasi_errno_t ret_alloc = __wasi_closure_allocate(&closure_index);
    __wasi_errno_t ret_free;

    assert(ret_alloc == 0);

    ret_free = __wasi_closure_free(closure_index);
    assert(ret_free == 0);
  }
}

static void test_index_zero(void) {
  __wasi_errno_t ret;

  printf("Test 5: closure_free with index 0\n");

  ret = __wasi_closure_free(0);
  assert(ret == 0);
}

static void test_max_index(void) {
  uint32_t max_index = 0xFFFFFFFFu;
  __wasi_errno_t ret;

  printf("Test 6: closure_free with u32::MAX\n");

  ret = __wasi_closure_free(max_index);
  assert(ret == 0);
}

static void test_multiple_allocations(void) {
  const int count = 5;
  __wasi_function_pointer_t indices[5];
  int i;
  int j;

  printf("Test 7: multiple allocations, free in reverse order\n");

  for (i = 0; i < count; i++) {
    __wasi_errno_t ret = __wasi_closure_allocate(&indices[i]);
    assert(ret == 0);
  }

  for (i = 0; i < count; i++) {
    for (j = i + 1; j < count; j++) {
      assert(indices[i] != indices[j]);
    }
  }

  for (i = count - 1; i >= 0; i--) {
    __wasi_errno_t ret = __wasi_closure_free(indices[i]);
    assert(ret == 0);
  }
}

int main(void) {
  printf("WASIX closure_free integration tests\n");
  test_invalid_index();
  test_allocate_and_free();
  test_double_free();
  test_multiple_cycles();
  test_index_zero();
  test_max_index();
  test_multiple_allocations();
  printf("All tests passed!\n");
  return 0;
}
