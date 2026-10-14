import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { scriptedModels } from "../src/faux.ts";
import {
    agents,
    changes,
    chat,
    joined,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import { fauxModels, scratch, setFolder } from "./support.ts";

it("names a wait's current chat without past and pins it only after the chief starts a new conversation (S5, C24)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await setFolder(run.home, scratch());
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Done."),
    ]);
    say(run.connection, "Run.");
    await vi.waitFor(() =>
        expect(changes(run.messages).some((item) => item.type === "wait")).toBe(
            true,
        ),
    );
    const waits = () =>
        changes(run.messages).flatMap((item) =>
            item.type === "wait" ? [item.wait] : [],
        );
    expect(waits().at(-1)?.chat).toEqual(chat);

    await request(run.connection, run.messages, {
        type: "new",
        id: randomUUID(),
        to: mayor,
    });

    await vi.waitFor(() =>
        expect(waits().at(-1)?.chat).toEqual({ with: mayor, past: 1 }),
    );
});

it("shows the budget and follows the mayor's thinking, fetching, failure and rest without opening its chat (S5)", async () => {
    const run = await served({
        env: { HAMLET_NET: "1", HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({
            tokensPerSecond: 20,
            responses: [
                {
                    thinking: "Let me see.",
                    calls: [
                        {
                            name: "bash",
                            arguments: {
                                command:
                                    "# curl example.invalid\nsleep 0.3; echo broken; exit 7",
                            },
                        },
                    ],
                },
                "Could not fetch.",
            ],
        }),
    });
    expect(run.state.hamlets[0]?.budget).toMatchObject({
        summary: "Hamlet today: $0.00 of $5.00",
        remains: "$5.00",
        left: 1,
    });
    expect(run.state.providers).toMatchObject([
        { id: "faux", configured: true },
    ]);
    say(run.connection, "Look.");
    await vi.waitFor(() =>
        expect(agents(run.messages)).toContainEqual(
            expect.objectContaining({
                busy: true,
                activity: {
                    doing: "think",
                    doingWords: "Thinking",
                    step: 1,
                    failures: 0,
                },
            }),
        ),
    );
    const late = await joined(run.server.url, run.home.credentials.token);
    expect(late.state.agents[0]).toMatchObject({
        busy: true,
        activity: { doing: "think", step: 1 },
    });
    await vi.waitFor(
        () => {
            expect(agents(run.messages)).toContainEqual(
                expect.objectContaining({
                    activity: {
                        doing: "fetch",
                        doingWords: "Fetching from the network",
                        step: 2,
                        failures: 0,
                    },
                    saying: { text: "", thought: true },
                }),
            );
            expect(agents(run.messages)).toContainEqual(
                expect.objectContaining({
                    activity: {
                        doing: "think",
                        doingWords: "Thinking",
                        step: 3,
                        failures: 1,
                    },
                }),
            );
            expect(agents(run.messages).at(-1)).toMatchObject({
                busy: false,
                activity: { doing: "rest", step: 0, failures: 0 },
                saying: { text: "Could not fetch.", thought: false },
            });
        },
        { timeout: 10_000 },
    );
    const revisions = run.messages.flatMap((message) =>
        message.type === "change" ? [message.revision] : [],
    );
    expect(
        revisions.every((revision, i) => revision > (revisions[i - 1] ?? 0)),
    ).toBe(true);
}, 20_000);
