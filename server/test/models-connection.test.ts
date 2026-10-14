import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { FAUX_MODEL, scriptedModels } from "../src/faux.ts";
import {
    agents,
    changes,
    joined,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import { openaiFake } from "./openai-fake.ts";
import { fauxModels } from "./support.ts";

it("sends grants from limits and answers and the link's testing and connected states (S5)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "limits",
            hamlet: "home",
            set: {
                section: "permissions",
                key: "unconfined",
                value: ["mayor"],
            },
        }),
    ).toMatchObject({ kind: "notice" });
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo once", network: true }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done."),
    ]);
    say(run.connection, "Once.");
    await vi.waitFor(() =>
        expect(
            changes(run.messages).some((change) => change.type === "wait"),
        ).toBe(true),
    );
    const wait = changes(run.messages).find((change) => change.type === "wait");
    if (wait?.type !== "wait") throw new Error("No wait");
    expect(
        await request(run.connection, run.messages, {
            type: "answer",
            id: randomUUID(),
            wait: wait.wait.id,
            choice: "always",
        }),
    ).toMatchObject({ kind: "notice" });
    await vi.waitFor(() =>
        expect(agents(run.messages).at(-1)).toMatchObject({
            busy: false,
            saying: { text: "Done." },
        }),
    );
    const late = await joined(run.server.url, run.home.credentials.token);
    expect(late.state.grants).toEqual([
        {
            agent: { hamlet: "home", number: 0 },
            name: "mayor",
            permission: "network",
            by: "chief",
            source: "grant",
        },
        {
            agent: { hamlet: "home", number: 0 },
            name: "mayor",
            permission: "unconfined",
            by: null,
            source: "limits",
        },
    ]);
    faux.setResponses([fauxAssistantMessage("Hello.")]);
    const start = run.messages.length;
    expect(
        await request(run.connection, run.messages, {
            type: "check",
            id: randomUUID(),
            what: "model",
        }),
    ).toMatchObject({ type: "reply", text: "Hello." });
    expect(
        changes(run.messages.slice(start))
            .filter((change) => change.type === "settings")
            .map((change) => change.settings.link.state),
    ).toEqual(["testing", "connected"]);
    expect(
        await request(run.connection, run.messages, {
            type: "check",
            id: randomUUID(),
            what: "connection",
        }),
    ).toMatchObject({ text: "Connected, 1 model(s)" });
});

it("reads and edits the catalogue and settings and forgets an endpoint's credential when it is removed", async () => {
    const endpoint = await openaiFake();
    const run = await served({
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
    });
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "catalogue",
            model: "local::org/large",
            set: {
                allowed: true,
                price: { input: 2, output: 3 },
                note: "For careful work.",
            },
        }),
    ).toMatchObject({ kind: "notice" });
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what: "models",
        }),
    ).toMatchObject({
        type: "read",
        models: expect.arrayContaining([
            {
                name: "local::org/large",
                price: { input: 2, output: 3 },
                priced: "yours",
                allowed: true,
                context: 32000,
                note: "For careful work.",
            },
        ]),
    });
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "catalogue",
            model: "local::org/large",
            set: { price: { input: -1, output: 3 } },
        }),
    ).toMatchObject({
        kind: "error",
        text: "a cost is a number of zero or more",
    });
    await run.home.credentials.modify("local", async () => ({
        type: "api_key",
        key: "local-secret",
    }));
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "settings",
            set: { endpoints: [], reflectEvery: 0, allowNet: true },
        }),
    ).toMatchObject({ kind: "notice" });
    expect(await run.home.credentials.read("local")).toBeUndefined();
    expect(
        changes(run.messages)
            .filter((change) => change.type === "settings")
            .at(-1),
    ).toMatchObject({
        settings: { endpoints: [], reflectEvery: 0, allowNet: true },
    });
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "settings",
            set: { reflectEvery: -1 },
        }),
    ).toMatchObject({
        kind: "error",
        text: "reflectEvery must be a whole number",
    });
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what: "prompt",
            agent: { ...mayor, hamlet: "missing" },
        }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: "no hamlet named `missing`",
    });
});

it("edits a listed built-in model's price, permission and note before the model has been used", async () => {
    const run = await served({ models: scriptedModels({ responses: [] }) });
    const result = await request(run.connection, run.messages, {
        type: "edit",
        id: randomUUID(),
        doc: "catalogue",
        model: FAUX_MODEL,
        set: {
            price: { input: 0.12, output: 0.34 },
            allowed: true,
            note: "Preferred.",
        },
    });
    expect(result).toMatchObject({ type: "reply", kind: "notice" });
    const listing = await request(run.connection, run.messages, {
        type: "read",
        id: randomUUID(),
        what: "models",
    });
    expect(listing).toMatchObject({
        type: "read",
        models: expect.arrayContaining([
            expect.objectContaining({
                name: FAUX_MODEL,
                price: { input: 0.12, output: 0.34 },
                priced: "yours",
                allowed: true,
                note: "Preferred.",
            }),
        ]),
    });
});
