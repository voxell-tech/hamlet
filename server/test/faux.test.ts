import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
    normalizeContext,
    type TranscriptContext,
} from "@earendil-works/pi-ai";
import { expect, it } from "vitest";
import { readScript, scriptedModels } from "../src/models.ts";
import { scratch } from "./support.ts";

it("keeps legacy scripts and refuses malformed agent queue keys", async () => {
    const file = join(scratch(), "faux.json");
    const legacy = {
        responses: ["Hello", { thinking: "Consider", text: "Yes" }],
        tokensPerSecond: 40,
    };
    await writeFile(file, JSON.stringify(legacy));
    expect(await readScript(file)).toEqual(legacy);
    await writeFile(
        file,
        JSON.stringify({
            ...legacy,
            agents: { "home:mayor": ["Never dispatched"] },
        }),
    );
    await expect(readScript(file)).rejects.toThrow("not a faux script");
});

function prompt(agent: number): TranscriptContext {
    return normalizeContext({
        messages: [
            {
                role: "system",
                content: "",
                timestamp: 0,
                sections: {
                    machine: `Workspace: /scratch/houses/home/${agent}`,
                },
            },
        ],
    });
}

it("dispatches concurrent faux requests without losing a response", async () => {
    const models = scriptedModels({
        agents: { "home:0": ["Mayor"], "home:1": ["Scout"] },
        responses: ["Fallback"],
    });
    const provider = models.getProvider("faux");
    const model = models.getModel("faux", "faux-1");
    if (!provider || !model) throw new Error("No faux model");
    const first = provider.streamSimple(model, prompt(0));
    const second = provider.streamSimple(model, prompt(1));
    const answers = await Promise.all([first.result(), second.result()]);
    expect(answers.map((answer) => answer.content)).toEqual([
        [{ type: "text", text: "Mayor" }],
        [{ type: "text", text: "Scout" }],
    ]);
    expect(
        (await provider.streamSimple(model, prompt(0)).result()).errorMessage,
    ).toBe("No more faux responses queued");
    expect(
        (await provider.streamSimple(model, prompt(2)).result()).content,
    ).toEqual([{ type: "text", text: "Fallback" }]);
});

it("routes from the machine section even when notes contain another house", async () => {
    const models = scriptedModels({
        agents: { "home:0": ["Mayor"], "home:1": ["Scout"] },
    });
    const provider = models.getProvider("faux");
    const model = models.getModel("faux", "faux-1");
    if (!provider || !model) throw new Error("No faux model");
    const context = prompt(0);
    context.messages.push({
        role: "system",
        content: "",
        timestamp: 1,
        sections: { notes: "Workspace: /scratch/houses/home/1" },
    });
    expect(
        (await provider.streamSimple(model, context).result()).content,
    ).toEqual([{ type: "text", text: "Mayor" }]);
});
