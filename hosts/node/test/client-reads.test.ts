import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";
import { test } from "node:test";
import { createModels } from "@earendil-works/pi-ai";
import { fauxModels } from "../src/agent/faux.ts";
import { modelList } from "../src/agent/models.ts";
import { servingEndpoints } from "../src/agent/providers.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import {
    A,
    B,
    bytes,
    part,
    profileAnswering,
    saves,
} from "./branch-testing.ts";
import { scratchFolder } from "./testing.ts";

const KEY = "sk-made-up-0d1e-papers";

const FAUX_WORDS = "Every agent runs the faux model here.";

test("a chief's memory edit is saved by the agent", async (t) => {
    const { calls, sent } = fakeCalls({
        profile: profileAnswering(
            part({ tip: A }),
            part({ tip: B }),
            part({ tip: B }),
        ),
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, { calls, files: fakeFiles(files) });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    const client = agent.by(from.client);

    assert.equal(
        await agent.actions.edit_memory(client, {
            path: "memory/notes.md",
            text: "Kept.",
        }),
        null,
    );
    assert.equal(files["memory/notes.md"], "Kept.");
    await agent.actions.edit_memory(client, {
        path: "memory/notes.md",
        text: "",
    });
    assert.equal(files["memory/notes.md"], undefined);
    assert.deepEqual(
        saves(sent).map((save) => save.changes),
        [
            [
                {
                    type: "write",
                    path: "memory/notes.md",
                    offset: 0,
                    bytes: bytes("Kept."),
                    last: true,
                },
            ],
            [{ type: "remove", path: "memory/notes.md" }],
        ],
    );
    for (const path of ["notes.md", "memory/../notes.md", "/memory/a.md"]) {
        await assert.rejects(
            agent.actions.edit_memory(client, { path, text: "Out." }),
            path,
        );
    }
    assert.equal(files["notes.md"], undefined);
    await agent.stop();
});

test("an agent's papers are its own files, learned against its profile's main", async (t) => {
    const { calls } = fakeCalls({
        profile: {
            ...profileAnswering(part({ tip: A }), part({ tip: B })),
            learned: () => ({
                character: "You write careful tests.",
                paths: [{ path: "memory/tests.md" }],
            }),
        },
    });
    const files: Record<string, string> = {};
    const agent = await wakeAgent(t, { calls, files: fakeFiles(files) });
    await agent.actions.fill(agent.by(from.realm), { profile: 2 });
    const memory = "---\ndescription: How the tests run\n---\nIn order.\n";
    files["memory/tests.md"] = memory;
    files["skills/run/SKILL.md"] = "---\ndescription: Runs a command\n---\n";
    files["notes.md"] = "Not a paper.";

    assert.deepEqual(await agent.actions.read_papers(agent.by(from.client)), {
        character: "You write careful tests.",
        learned: ["memory/tests.md"],
        memory: [
            {
                description: "How the tests run",
                path: "tests.md",
                text: memory,
            },
        ],
        skills: [{ description: "Runs a command", name: "run" }],
    });
    await agent.stop();
});

test("on a faux script every check answers that the faux model runs, and the faux models are listed", async (t) => {
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        faux: true,
    });
    const client = agent.by(from.client);

    assert.deepEqual(
        await agent.actions.check(client, { model: "faux::faux-1" }),
        { type: "answered", words: FAUX_WORDS },
    );
    assert.deepEqual(await agent.actions.check_connection(client), {
        type: "answered",
        words: FAUX_WORDS,
    });
    const { models } = await agent.actions.read_models(client, {});
    assert.deepEqual(
        models.map((model) => model.name),
        ["faux::faux-1"],
    );
    await agent.stop();
});

test("on a faux script a line gives the model up to twenty images", async (t) => {
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        faux: true,
    });

    assert.deepEqual(await agent.actions.images_taken(agent.by(from.client)), {
        per_line: 20,
    });
    await agent.stop();
});

test("the Hand lists its providers in order, in their words", async (t) => {
    const { calls } = fakeCalls({
        realm: {
            credential: ({ provider }) =>
                provider === "loopback"
                    ? { type: "key", key: KEY }
                    : {
                          type: "refused",
                          why: { type: "not_signed_in", provider },
                          words: `nothing is signed in to ${provider}`,
                      },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        models: () => fauxModels(undefined, true).models,
    });

    const { providers } = await agent.actions.read_providers(
        agent.by(from.client),
    );
    assert.deepEqual(
        providers.map(({ id, label, methods, configured }) => ({
            id,
            label,
            methods,
            configured,
        })),
        [
            {
                id: "chatgpt",
                label: "Continue with ChatGPT",
                methods: ["subscription"],
                configured: false,
            },
            { id: "faux", label: "faux", methods: [], configured: false },
            {
                id: "loopback",
                label: "loopback",
                methods: ["key"],
                configured: true,
            },
        ],
    );
    await agent.stop();
});

test("a faux model is titled by its id and takes images", (t) => {
    const script = join(scratchFolder(t), "script.json");
    writeFileSync(
        script,
        JSON.stringify({ models: ["faux-a", { id: "faux-b", window: 32000 }] }),
    );
    const titles = (path: string | undefined) =>
        modelList(fauxModels(path, false).models).map(
            ({ name, title, images }) => ({ name, title, images }),
        );

    assert.deepEqual(titles(undefined), [
        { name: "faux::faux-1", title: "faux-1", images: {} },
    ]);
    assert.deepEqual(titles(script), [
        { name: "faux::faux-a", title: "faux-a", images: {} },
        { name: "faux::faux-b", title: "faux-b", images: {} },
    ]);
});

test("a faux answer books as input every prompt token the cache did not read, and counts it once", async () => {
    const { models, model } = fauxModels(undefined, false);
    const [provider = "", id = ""] = model.split("/");
    const faux = models.getModel(provider, id);
    assert.ok(faux !== undefined);
    const asked = (...lines: string[]) => ({
        messages: lines.map((content) => ({
            role: "user" as const,
            content,
            timestamp: 0,
        })),
    });
    const usage = async (
        context: ReturnType<typeof asked>,
        sessionId?: string,
    ) =>
        (
            await models.completeSimple(
                faux,
                context,
                sessionId === undefined ? {} : { sessionId },
            )
        ).usage;
    const first = asked("Read the papers.");
    const second = asked("Read the papers.", "Now the notes.");
    const alone = [await usage(first), await usage(second)];
    const once = await usage(first, "session-1");
    const again = await usage(second, "session-1");
    assert.ok(alone[0] !== undefined && alone[0].input > 0);
    assert.deepEqual(
        [once.input + once.cacheRead, again.input + again.cacheRead],
        alone.map(({ input }) => input),
    );
    assert.ok(again.cacheRead > 0);
    // Each prompt token is counted once, as with no cache.
    assert.deepEqual(
        [once.totalTokens, again.totalTokens],
        alone.map(({ totalTokens }) => totalTokens),
    );
});

test("a read of the models with refresh asks each endpoint for its list again first", async (t) => {
    let listed = ["org/small"];
    const server = createServer((_request, response) => {
        response.setHeader("content-type", "application/json");
        response.end(JSON.stringify({ data: listed.map((id) => ({ id })) }));
    });
    await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
    t.after(() => server.close());
    const base_url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
    const models = createModels();
    servingEndpoints(models)({
        settings: {
            reflect_every: 0,
            allow_net: false,
            endpoints: [{ name: "local", base_url }],
        },
    });
    await until(() => models.getModel("local", "org/small") !== undefined);
    const { calls } = fakeCalls({
        realm: {
            credential: ({ provider }) => ({
                type: "refused",
                why: { type: "not_signed_in", provider },
                words: `nothing is signed in to ${provider}`,
            }),
        },
    });
    const agent = await wakeAgent(t, { calls, models: () => models });
    const client = agent.by(from.client);
    const names = async (refresh?: boolean) =>
        (
            await agent.actions.read_models(
                client,
                refresh === undefined ? {} : { refresh },
            )
        ).models.map(({ name }) => name);

    listed = ["org/large"];
    assert.deepEqual(await names(), ["local::org/small"]);
    assert.deepEqual(await names(true), ["local::org/large"]);
    await agent.stop();
});
