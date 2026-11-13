// Hosts one pi() agent on the native module for `pi-kill.test.ts`, and
// writes what happens to standard output, one JSON object a line.
//
// node test/native/pi-kill-child.ts <dir> <replay> <mode>
//
// `replay` is the `slow` tool's replay setting, `safe` or `unsafe`. In mode
// `kill`, the process prompts the agent, and `slow` kills the process with
// SIGKILL once it has started. In mode `resume`, it starts the host on the
// same folder, waits a while for anything to run by itself, and then sends
// the same prompt with the same request id.
import { setTimeout as sleep } from "node:timers/promises";
import {
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai";
import {
    createRegistry,
    defineExtension,
    defineTool,
} from "@earendil-works/pi-durable";
import { pi } from "../../src/pi/actor.ts";
import { fauxModels, startHost } from "../testing.ts";

const [dir, replay, mode] = process.argv.slice(2);
if (
    dir === undefined ||
    (replay !== "safe" && replay !== "unsafe") ||
    (mode !== "kill" && mode !== "resume")
) {
    throw new Error(
        "usage: pi-kill-child.ts <dir> <safe|unsafe> <kill|resume>",
    );
}
const say = (event: Record<string, unknown>) =>
    process.stdout.write(`${JSON.stringify(event)}\n`);

// The model calls `slow` until it sees a tool result, then answers with it.
const callOrAnswer: FauxResponseFactory = (transcript) => {
    const last = transcript.messages.at(-1);
    if (last?.role !== "toolResult") {
        say({ event: "model", answered: false });
        return fauxAssistantMessage([fauxToolCall("slow", {})], {
            stopReason: "toolUse",
        });
    }
    const text = last.content
        .map((part) => (part.type === "text" ? part.text : ""))
        .join("");
    say({ event: "model", answered: true, isError: last.isError, text });
    return fauxAssistantMessage(`the tool said: ${text}`);
};
const { faux, models, model } = fauxModels();
faux.setResponses(Array.from({ length: 8 }, () => callOrAnswer));

const slow = defineTool({
    name: "slow",
    description: "Takes a while.",
    parameters: { type: "object", properties: {} },
    replay,
    execute: async (_args, api) => {
        say({ event: "tool", call: api.callId });
        if (mode === "kill") process.kill(process.pid, "SIGKILL");
        return { output: [{ type: "text", text: "slow is done" }] };
    },
});
const registry = createRegistry();
registry.install(defineExtension({ name: "slow", tools: [slow] }));
const agent = pi({
    registry,
    models,
    model,
});

const host = await startHost(dir, { agent });
say({ event: "started", due: host.due });
if (mode === "resume") {
    await sleep(3_000);
    say({ event: "waited", modelCalls: faux.state.callCount });
}
const answer = await host.call("agent", "a1", "prompt", "Run slow.", {
    requestId: "prompt-1",
});
say({ event: "answer", answer, modelCalls: faux.state.callCount });
await host.shutdown();
// The native module keeps Node's event loop alive after the host stops.
process.exit(0);
