import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { parseServer } from "../src/protocol.ts";
import { RealmDoc } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { changes, joined, served } from "./connection-support.ts";
import {
    addAgent,
    addHamlet,
    context,
    fauxAgents,
    fauxModels,
    say,
} from "./support.ts";

it("shows all hamlets, office and built roads, departed agents and closed hamlets", async () => {
    const { models } = fauxModels();
    const { home, server, messages } = await served({ models });
    await addAgent(home, "home", "scout");
    await addAgent(home, "home", "smith");
    await addHamlet(home, "mill", "miller");
    await home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, "home", "home");
        record.roads.push({
            a: 1,
            b: 2,
            messages: 4,
            lastFrom: 1,
            lastUsed: home.now(),
            since: home.now(),
            by: "mayor",
        });
        record.archive.push({
            id: "left",
            agent: {
                number: 3,
                name: "cook",
                purpose: "Cooked",
                since: 1,
                model: null,
            },
            how: "merged",
            into: 2,
            at: home.now(),
            by: "chief:alice",
            reason: "Same work",
            roads: [],
            merge: null,
            back: null,
        });
        const realm = await tx.doc(RealmDoc);
        const mill = realm.hamlets.find((h) => h.id === "mill");
        if (mill) {
            mill.title = "The Mill";
            mill.at = { x: 4, z: 7 };
        }
        realm.hamlets.push({
            id: "closed",
            title: "Old Town",
            at: null,
            chiefs: [home.chief],
            from: null,
            founded: 1,
            removed: { at: 2, by: "chief:alice", reason: "Done" },
            erased: false,
        });
    }, context);
    const late = await joined(server.url, home.credentials.token);
    expect(late.state.hamlets).toMatchObject([
        { id: "home", roads: expect.any(Array) },
        { id: "mill", name: "The Mill", at: { x: 4, z: 7 } },
    ]);
    expect(late.state.hamlets[0]?.roads).toContainEqual(
        expect.objectContaining({
            a: { hamlet: "home", number: 1 },
            b: { hamlet: "home", number: 2 },
            messages: 4,
            level: "paved",
            lastFrom: { hamlet: "home", number: 1 },
        }),
    );
    expect(late.state.hamlets[0]?.roads).toContainEqual(
        expect.objectContaining({
            a: { hamlet: "home", number: 0 },
            b: { hamlet: "mill", number: 0 },
        }),
    );
    expect(
        late.state.agents.find((a) => a.name === "cook")?.left,
    ).toMatchObject({
        how: "merged",
        into: { hamlet: "home", number: 2 },
        by: "the chief alice",
        words: expect.stringContaining("talk to `smith`"),
    });
    expect(late.state.closed).toEqual([
        {
            id: "closed",
            name: "Old Town",
            at: 2,
            by: "the chief alice",
            reason: "Done",
        },
    ]);
    expect(late.state.library).toEqual([]);
    for (const frame of [...messages, ...late.messages])
        expect(parseServer(JSON.stringify(frame))).toEqual({ message: frame });
});

it("follows unopened peer conversations, traffic and the peer list", async () => {
    let release = () => {};
    const waiting = new Promise<void>((resolve) => {
        release = resolve;
    });
    const { models, faux } = fauxModels();
    const { home, messages } = await served({
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await addAgent(home, "home", "scout");
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", { name: "scout", message: "Look" }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done"),
        ],
        "home:1": [
            async () => {
                await waiting;
                return fauxAssistantMessage("Looked");
            },
        ],
    });
    const sub = await say(home, "Ask");
    try {
        await vi.waitFor(() =>
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "agent",
                    agent: expect.objectContaining({
                        name: "scout",
                        busy: true,
                        conversations: [
                            {
                                with: { hamlet: "home", number: 0 },
                                kind: "questions",
                                count: 1,
                                last: expect.any(Number),
                            },
                        ],
                    }),
                }),
            ),
        );
        await vi.waitFor(() =>
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({
                        roads: expect.arrayContaining([
                            expect.objectContaining({
                                messages: 1,
                                lastFrom: { hamlet: "home", number: 0 },
                            }),
                        ]),
                    }),
                }),
            ),
        );
    } finally {
        release();
    }
    await sub.wait(context);
});

it("sends a whole state when a hamlet closes and reconciles for a late client", async () => {
    const { models } = fauxModels();
    const { home, server, messages } = await served({ models });
    await addHamlet(home, "mill", "miller");
    await vi.waitFor(() =>
        expect(changes(messages)).toContainEqual(
            expect.objectContaining({
                type: "hamlet",
                hamlet: expect.objectContaining({ id: "mill" }),
            }),
        ),
    );
    await home.harness.commit(async (tx) => {
        const mill = (await tx.doc(RealmDoc)).hamlets.find(
            (h) => h.id === "mill",
        );
        if (mill)
            mill.removed = {
                at: home.now(),
                by: "chief:chief",
                reason: "Done",
            };
    }, context);
    await vi.waitFor(() =>
        expect(messages.filter((m) => m.type === "state").at(-1)).toMatchObject(
            { hamlets: [{ id: "home" }], closed: [{ id: "mill" }] },
        ),
    );
    const late = await joined(server.url, home.credentials.token);
    expect(late.state.closed).toMatchObject([{ id: "mill" }]);
    expect(late.state.hamlets).toHaveLength(1);
});

it("replaces the whole state when the library view changes", async () => {
    const library = await import("../src/society/librarian.ts");
    const { LibraryDoc } = await import("../src/society/docs.ts");
    const view = vi
        .spyOn(library, "libraryView")
        .mockImplementation(async (home, context) => {
            const doc = await home.harness.snapshot(LibraryDoc, context);
            return (doc?.events ?? [])
                .filter((e) => e.kind === "shelved")
                .map((e) => ({
                    name: e.skill,
                    description: "Shared skill",
                    by: e.by,
                }));
        });
    try {
        const { models } = fauxModels();
        const { home, server, messages } = await served({ models });
        await home.harness.commit(async (tx) => {
            (await tx.doc(LibraryDoc)).events.push({
                kind: "shelved",
                skill: "testing",
                by: "scout@home",
                reason: "Useful",
                at: home.now(),
            });
        }, context);
        await vi.waitFor(() =>
            expect(
                messages.filter((m) => m.type === "state").at(-1),
            ).toMatchObject({
                library: [{ name: "testing", by: "scout@home" }],
                revision: expect.any(Number),
            }),
        );
        const late = await joined(server.url, home.credentials.token);
        expect(late.state.library).toEqual([
            { name: "testing", description: "Shared skill", by: "scout@home" },
        ]);
    } finally {
        view.mockRestore();
    }
});

it("does not offer a chief-chat retry for a failed peer conversation", async () => {
    const { ChatDoc } = await import("../src/chats.ts");
    const { models, faux } = fauxModels();
    const { home, server } = await served({ models });
    await addAgent(home, "home", "scout");
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", { name: "scout", message: "Look" }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done"),
        ],
        "home:1": [
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: "Could not answer",
            }),
        ],
    });
    await (await say(home, "Ask")).wait(context);
    const late = await joined(server.url, home.credentials.token);
    expect(late.state.agents.find((a) => a.name === "scout")).toMatchObject({
        busy: false,
        faulted: false,
    });
    expect(
        (await home.harness.snapshot(ChatDoc, "home:1", context))?.chats[
            home.chief
        ],
    ).toBeUndefined();
});
