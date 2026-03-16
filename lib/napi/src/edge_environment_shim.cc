#include "edge_environment.h"

#include "unofficial_napi.h"

namespace edge {

void Environment::AssignToContext(void* token) {
  (void)token;
}

void Environment::UnassignFromContext(void* token) {
  (void)token;
}

}  // namespace edge

edge::Environment* EdgeEnvironmentGet(napi_env env) {
  return static_cast<edge::Environment*>(unofficial_napi_get_edge_environment(env));
}

void EdgeEnvironmentDetach(napi_env env) {
  (void)unofficial_napi_set_edge_environment(env, nullptr);
}

void EdgeEnvironmentRunCleanup(napi_env env) {
  (void)env;
}

void EdgeEnvironmentRunAtExitCallbacks(napi_env env) {
  (void)env;
}
