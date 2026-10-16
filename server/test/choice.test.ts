import {
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import type { Tx } from "@earendil-works/pi-durable";
import { describe, expect, it, vi } from "vitest";
import type { Home } from "../src/home.ts";
import { defaultLimits, LimitsDoc } from "../src/limits.ts";
import { choose, commitEdit } from "../src/models.ts";
import { HOME, RecordDoc } from "../src/record.ts";
import { SettingsDoc } from "../src/settings.ts";
import {
    addHamlet,
    CHIEF,
    chatWith,
    context,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
} from "./support.ts";

/** Makes `change` through commitEdit, as the chief. */
function edit(
    home: Home,
    change: (tx: Tx) => Promise<void>,
    reason?: string,
): Promise<void> {
    return home.harness.commit(
        (tx) =>
            commitEdit(home, tx, {
                by: CHIEF,
                change,
                ...(reason === undefined ? {} : { reason }),
            }),
        context,
    );
}

const limits = (tx: Tx) => tx.doc(LimitsDoc, HOME, HOME);

describe("the mayor's model", () => {
    it("is the chief's pin, else the hamlet's model, else the home's default", async () => {
        const { models, faux } = fauxModels(["faux-1", "faux-2", "faux-3"]);
        // No override, so the home's default is its stored settings'.
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_MODEL: undefined },
        });
        await mayorChat(home);
        const seen: string[] = [];
        const answer: FauxResponseFactory = (_t, _o, _s, model) => {
            seen.push(model.id);
            return fauxAssistantMessage("Yes.");
        };
        faux.setResponses(Array(5).fill(answer));
        const ask = async () =>
            (await (await say(home, "Which?")).wait(context)).status;

        await edit(home, async (tx) => {
            (await tx.doc(SettingsDoc)).model = "faux::faux-1";
        });
        expect(await ask()).toBe("done");
        await edit(home, async (tx) => {
            (await limits(tx)).model = "faux::faux-2";
        });
        expect(await ask()).toBe("done");
        await edit(home, async (tx) => {
            (await limits(tx)).pinned.mayor = "faux::faux-3";
        });
        expect(await ask()).toBe("done");
        await edit(home, async (tx) => {
            (await limits(tx)).model = "faux::faux-1";
        });
        expect(await ask()).toBe("done");
        await edit(home, async (tx) => {
            const own = await limits(tx);
            delete own.pinned.mayor;
        });
        expect(await ask()).toBe("done");

        expect(seen).toEqual([
            "faux-1",
            "faux-2",
            "faux-3",
            "faux-3",
            "faux-1",
        ]);
    });

    it("is the default for an agent named like a property every object has", () => {
        const settings = SettingsDoc.definition.initial();
        const agent = { name: "constructor", mayor: false };

        expect(choose(defaultLimits(), settings, agent)).toBe(settings.model);
        const limits = defaultLimits();
        limits.pinned = Object.fromEntries([["constructor", "faux::faux-2"]]);
        expect(choose(limits, settings, agent)).toBe("faux::faux-2");
    });

    it("changes from the next request of a run that is going", async () => {
        const { models, faux } = fauxModels(["faux-1", "faux-2"]);
        const home = await testHome(scratch(), { models });
        const seen: string[] = [];
        faux.setResponses([
            async (_t, _o, _s, model) => {
                seen.push(model.id);
                await edit(home, async (tx) => {
                    (await limits(tx)).pinned.mayor = "faux::faux-2";
                });
                return fauxAssistantMessage(
                    fauxToolCall("bash", { command: "true" }),
                    { stopReason: "toolUse" },
                );
            },
            (_t, _o, _s, model) => {
                seen.push(model.id);
                return fauxAssistantMessage("Done.");
            },
        ]);

        expect((await (await say(home, "Go.")).wait(context)).status).toBe(
            "done",
        );

        expect(seen).toEqual(["faux-1", "faux-2"]);
    });

    it("is recorded at each change, with who chose it and why", async () => {
        let now = 10;
        const { models } = fauxModels(["faux-1", "faux-2"]);
        const home = await testHome(scratch(), { models, now: () => now });

        await edit(home, async (tx) => {
            (await limits(tx)).pinned.mayor = "faux::faux-2";
        });
        now = 20;
        await edit(home, async (tx) => {
            (await limits(tx)).model = "faux::faux-1";
        });
        now = 30;
        await edit(
            home,
            async (tx) => {
                const own = await limits(tx);
                delete own.pinned.mayor;
                own.model = null;
                (await tx.doc(SettingsDoc)).model = "far::large";
            },
            "The chief moved the home to far.",
        );

        const record = await home.harness.snapshot(RecordDoc, HOME, context);
        expect(record?.decisions).toEqual([
            {
                kind: "model",
                at: 10,
                by: CHIEF,
                agent: 0,
                model: "faux::faux-2",
                reason: "The chief chose it.",
            },
            {
                kind: "model",
                at: 30,
                by: CHIEF,
                agent: 0,
                // HAMLET_MODEL still names the home's default.
                model: "faux::faux-1",
                reason: "The chief moved the home to far.",
            },
        ]);
    });

    it("follows an override that changed between two starts", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels(["faux-1", "faux-2"]);
        const first = await testHome(dir, { models });
        await mayorChat(first);
        await first.close(context);
        const seen: string[] = [];
        faux.setResponses([
            (_t, _o, _s, model) => {
                seen.push(model.id);
                return fauxAssistantMessage("Yes.");
            },
        ]);

        const home = await testHome(dir, {
            models,
            env: { HAMLET_MODEL: "faux::faux-2" },
        });
        expect((await (await say(home, "Which?")).wait(context)).status).toBe(
            "done",
        );

        expect(seen).toEqual(["faux-2"]);
    });

    it("keeps a fifth of its window free for the answer", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models });

        // The faux model holds 128,000 tokens.
        expect(home.compaction.reserveTokens).toBe(25_600);
        await edit(home, async (tx) => {
            (await limits(tx)).pinned.mayor = "far::unknown";
        });
        // pi-durable's default, for a model whose window is not known.
        await vi.waitFor(() =>
            expect(home.compaction.reserveTokens).toBe(16_384),
        );
    });
});

it("uses the mayor's allowed choice and reconfigures another hamlet when that allowance is revoked", async () => {
    const { models, faux } = fauxModels(["faux-1", "faux-2", "faux-3"]);
    const home = await testHome(scratch(), { models });
    const miller = await addHamlet(home, "mill", "miller");
    await edit(home, async (tx) => {
        const limits = await tx.doc(LimitsDoc, "mill", "mill");
        limits.models = ["faux::faux-2"];
        limits.model = "faux::faux-3";
        const agent = (await tx.doc(RecordDoc, "mill", "mill")).agents[0];
        if (!agent) throw new Error("no miller");
        agent.model = "faux::faux-2";
    });
    const chat = await chatWith(home, miller);
    const seen: string[] = [];
    const step: FauxResponseFactory = (_prompt, _options, _state, model) => {
        seen.push(model.id);
        return fauxAssistantMessage("done");
    };
    faux.setResponses([step, step]);
    await (await chat.submit({ type: "input", content: "work" }, context)).wait(
        context,
    );
    await edit(home, async (tx) => {
        (await tx.doc(LimitsDoc, "mill", "mill")).models = [];
    });
    await (
        await chat.submit({ type: "input", content: "work again" }, context)
    ).wait(context);
    expect(seen).toEqual(["faux-2", "faux-3"]);
});
