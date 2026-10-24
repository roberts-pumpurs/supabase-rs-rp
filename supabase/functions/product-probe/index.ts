// Local acceptance endpoint for buffered and streamed Edge Function responses.
Deno.serve(async (request: Request) => {
  if (request.method !== "POST") {
    return new Response("method not allowed", { status: 405 });
  }
  const input = await request.json();
  if (input.mode === "stream") {
    const encoder = new TextEncoder();
    const stream = new ReadableStream({
      async start(controller) {
        for (let sequence = 0; sequence < 3; sequence += 1) {
          controller.enqueue(encoder.encode(JSON.stringify({ sequence }) + "\n"));
          const { promise, resolve } = Promise.withResolvers<void>();
          setTimeout(resolve, 10);
          await promise;
        }
        controller.close();
      },
    });
    return new Response(stream, { headers: { "content-type": "application/x-ndjson" } });
  }
  if (typeof input.left !== "number" || typeof input.right !== "number") {
    return Response.json({ error: "left and right must be numbers" }, { status: 400 });
  }
  return Response.json({ sum: input.left + input.right });
});
