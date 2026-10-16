import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import type { Address, State } from "../src/protocol.ts";
import { request, served } from "./connection-support.ts";
import { fauxAgents, fauxModels } from "./support.ts";

/** A society served over a real loopback WebSocket, with per-agent faux models. */
export async function society() {
    const { models, faux } = fauxModels(["faux-1"], 100);
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    return { ...run, faux };
}

/** A model's next tool call. */
export function tool(name: string, args: Parameters<typeof fauxToolCall>[1]) {
    return fauxAssistantMessage(fauxToolCall(name, args), {
        stopReason: "toolUse",
    });
}

/** Sends a chief's line and waits for its addressed receipt. */
export function deed(
    run: Awaited<ReturnType<typeof society>>,
    text: string,
    to: Address = { hamlet: "home", agent: "mayor" },
) {
    return request(run.connection, run.messages, {
        type: "say",
        id: randomUUID(),
        to,
        text,
    });
}

/** Reads the current chief conversation through the public protocol. */
export async function read(
    run: Awaited<ReturnType<typeof society>>,
    to: Address = { hamlet: "home", agent: "mayor" },
) {
    const result = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: { with: to },
    });
    if (result.type !== "transcript")
        throw new Error(`No transcript: ${JSON.stringify(result)}`);
    return result;
}

/** Creates the mayor's three voters through real tools, including the scout-smith road. */
export async function agents(run: Awaited<ReturnType<typeof society>>) {
    fauxAgents(run.faux, {
        "home:0": [
            tool("create_agent", {
                name: "scout",
                purpose: "Scout",
                reason: "Explore",
            }),
            tool("create_agent", {
                name: "smith",
                purpose: "Smith",
                reason: "Build",
                roads: ["scout"],
            }),
            tool("create_agent", {
                name: "poet",
                purpose: "Poet",
                reason: "Write",
            }),
            fauxAssistantMessage("The society is ready"),
        ],
    });
    await deed(run, "Build our society");
}

/** A groundedness-optional yes ballot, with H13's required reason. */
export function yes() {
    return tool("cast_ballot", {
        choice: "yes",
        reason: "A new direction",
        evidence: "",
    });
}

/** Undirected road ends, normalized only for assertions. */
export function roadEnds(state: State) {
    return state.hamlets.flatMap((h) =>
        (h.roads ?? []).map((r) =>
            [`${r.a.hamlet}#${r.a.number}`, `${r.b.hamlet}#${r.b.number}`]
                .sort()
                .join("--"),
        ),
    );
}
