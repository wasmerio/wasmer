#ifndef EDGE_INTERNAL_BINDING_BINDING_MESSAGING_H_
#define EDGE_INTERNAL_BINDING_BINDING_MESSAGING_H_

#include <memory>

#include "node_api.h"

namespace internal_binding {

struct MessagePortData;
using MessagePortDataPtr = std::shared_ptr<MessagePortData>;
using EdgeMessagePortData = MessagePortData;
using EdgeMessagePortDataPtr = MessagePortDataPtr;

EdgeMessagePortDataPtr EdgeCreateMessagePortData();
void EdgeEntangleMessagePortData(const EdgeMessagePortDataPtr& first,
                                const EdgeMessagePortDataPtr& second);
EdgeMessagePortDataPtr EdgeGetMessagePortData(napi_env env, napi_value value);
napi_value EdgeCreateMessagePortForData(napi_env env, const EdgeMessagePortDataPtr& data);
void EdgeCloseMessagePortForValue(napi_env env, napi_value value);

}  // namespace internal_binding

#endif  // EDGE_INTERNAL_BINDING_BINDING_MESSAGING_H_
