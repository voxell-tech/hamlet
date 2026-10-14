import { createServer } from "node:http";
import { closeAfter } from "./support.ts";

/** A loopback-only OpenAI-compatible endpoint for offline tests. */
export async function openaiFake() {
    const requests: Record<string, unknown>[] = [];
    const keys: (string | undefined)[] = [];
    let listing: unknown = {
        data: [{ id: "org/large", context_length: 32_000 }],
    };
    let usage: unknown = {
        prompt_tokens: 100,
        completion_tokens: 20,
        prompt_tokens_details: { cached_tokens: 30 },
        completion_tokens_details: { reasoning_tokens: 5 },
        cost: 0.0015,
    };
    let failures = 0;
    let held = false;
    let release: (() => void) | undefined;
    let answered = 0;
    const server = createServer(async (request, response) => {
        keys.push(request.headers.authorization);
        if (request.url === "/v1/models") {
            response.setHeader("Content-Type", "application/json");
            response.end(JSON.stringify(listing));
            return;
        }
        let text = "";
        for await (const chunk of request) text += String(chunk);
        requests.push(JSON.parse(text) as Record<string, unknown>);
        if (failures-- > 0) {
            response.writeHead(503, { "Retry-After": "0" });
            response.end("busy");
            return;
        }
        if (held)
            await new Promise<void>((resolve) => {
                release = resolve;
                response.on("close", resolve);
            });
        if (response.destroyed) return;
        response.writeHead(200, { "Content-Type": "text/event-stream" });
        const chunk = (value: unknown) =>
            response.write(`data: ${JSON.stringify(value)}\n\n`);
        chunk({
            id: `call-${++answered}`,
            choices: [
                {
                    index: 0,
                    delta: { role: "assistant", content: "Hello." },
                    finish_reason: null,
                },
            ],
        });
        chunk({ choices: [{ index: 0, delta: {}, finish_reason: "stop" }] });
        if (usage !== undefined) chunk({ choices: [], usage });
        response.end("data: [DONE]\n\n");
    });
    await new Promise<void>((resolve) =>
        server.listen(0, "127.0.0.1", resolve),
    );
    const address = server.address();
    if (address === null || typeof address === "string")
        throw new Error("no listener");
    const close = async () => {
        server.closeAllConnections();
        await new Promise<void>((resolve) => server.close(() => resolve()));
    };
    closeAfter(close);
    return {
        url: `http://127.0.0.1:${address.port}/v1`,
        requests,
        keys,
        close,
        listing: (value: unknown) => {
            listing = value;
        },
        usage: (value: unknown) => {
            usage = value;
        },
        fail: (count: number) => {
            failures = count;
        },
        hold: () => {
            held = true;
        },
        release: () => {
            held = false;
            release?.();
        },
    };
}
