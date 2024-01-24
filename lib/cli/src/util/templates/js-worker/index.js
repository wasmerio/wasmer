
// Handler function.
// Receives a request and returns a response.
async function handleRequest(ev) {
  const request = ev.request;

  const out = JSON.stringify({
    success: true,
    method: request.method,
    package: "{{package}}",
  });
  return new Response(out, {
    headers: { "content-type": "application/json" },
  });
}

// Register the listener that handles incoming requests.
addEventListener("fetch", (event) => {
  event.respondWith(handleRequest(event));
});
