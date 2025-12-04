#include <assert.h>
#include <stdio.h>
#include <stdexcept>

#include <wasix/context.h>

namespace {

class ContextFailure : public std::exception {
 public:
  explicit ContextFailure(int code) : code_(code) {}

  const char *what() const noexcept override { return "ContextFailure"; }

  int code() const noexcept { return code_; }

 private:
  int code_;
};

wasix_context_id_t ctx_throw_unhandled;
bool main_caught_exception = false;

void context_entry_unhandled() { throw ContextFailure(7); }

} // namespace

int main() {
  int ret = wasix_context_create(&ctx_throw_unhandled, context_entry_unhandled);
  assert(ret == 0 && "failed to create throwing context");

  try {
    wasix_context_switch(ctx_throw_unhandled);
    assert(!"context switch returned without propagating exception");
  } catch (const ContextFailure &err) {
    main_caught_exception = true;
    assert(err.code() == 7);
  } catch (...) {
    assert(!"unexpected exception type propagated");
  }

  ret = wasix_context_destroy(ctx_throw_unhandled);
  assert(ret == 0 && "failed to destroy throwing context");

  assert(main_caught_exception);

  fprintf(stderr, "context_exception_to_main passed\n");
  return 0;
}
