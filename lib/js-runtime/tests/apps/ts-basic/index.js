export function handler(request) {
  const url = new URL(request.url);
  const body = `hello from js-runtime: ${url.pathname}`;
  return new Response(body, {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
}
