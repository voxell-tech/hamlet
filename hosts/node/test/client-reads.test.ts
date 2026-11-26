import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxModels } from "../src/agent/faux.ts";
import { fakeCalls, fakeFiles, from, wakeAgent } from "./agent-testing.ts";
import {
    A,
    B,
    bytes,
    part,
    profileAnswering,
    saves,
} from "./branch-testing.ts";

/** Made up; no provider has ever issued it. */
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

test("on a faux script every check answers that the faux model runs, and no model is listed", async (t) => {
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
    assert.deepEqual(await agent.actions.read_models(client), { models: [] });
    await agent.stop();
});

test("the Hand lists its providers in today's order and words", async (t) => {
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
