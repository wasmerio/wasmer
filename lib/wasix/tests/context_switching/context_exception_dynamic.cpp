#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdexcept>

#include <wasix/call_dynamic.h>
#include <wasix/context.h>

namespace {

class TaggedRuntimeError : public std::runtime_error {
 public:
  explicit TaggedRuntimeError(const char *message)
      : std::runtime_error(message) {}
};

wasix_context_id_t ctx_dynamic;
bool dynamic_call_executed = false;
bool exception_caught_locally = false;

extern "C" void throw_through_dynamic(void) {
  dynamic_call_executed = true;
  throw TaggedRuntimeError("dynamic failure");
}

void context_entry_dynamic() {
  try {
    wasix_function_pointer_t fn =
        reinterpret_cast<wasix_function_pointer_t>(&throw_through_dynamic);
    (void)wasix_call_dynamic(fn, nullptr, 0, nullptr, 0, true);
    assert(!"wasix_call_dynamic returned without throwing");
  } catch (const TaggedRuntimeError &err) {
    (void)err;
    exception_caught_locally = true;
  } catch (...) {
    assert(!"unexpected exception type");
  }

  wasix_context_switch(wasix_context_main);
}

} // namespace

int main() {
  int ret = wasix_context_create(&ctx_dynamic, context_entry_dynamic);
  assert(ret == 0 && "failed to create dynamic context");

  wasix_context_switch(ctx_dynamic);

  ret = wasix_context_destroy(ctx_dynamic);
  assert(ret == 0 && "failed to destroy dynamic context");

  assert(dynamic_call_executed);
  assert(exception_caught_locally);

  fprintf(stderr, "context_exception_dynamic passed\n");
  return 0;
}
