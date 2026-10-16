import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { ChiefEntry, NoticeEntry, RetryEntry } from "../src/chats.ts";
import { RecordDoc } from "../src/record.ts";
import {
    agents,
    changes,
    chat,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import {
    context,
    fauxModels,
    request as modelRequest,
    scratch,
    setFolder,
} from "./support.ts";

it("shows a held edit's change and words, runs only after allow, and lets a chief's pin and papers reach the same run's next request", async () => {
    const { models, faux } = fauxModels(["faux-1", "faux-2"]);
    const run = await served({
        models,
        env: {
            HAMLET_REFLECT_EVERY: "0",
            HAMLET_API_KEY: "sk-chief-model-private",
        },
    });
    await setFolder(run.home, scratch());
    let second: ReturnType<typeof modelRequest> | undefined;
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("write", { path: "a.txt", content: "one" }),
            { stopReason: "toolUse" },
        ),
        (input, _options, _state, model) => {
            second = modelRequest(input.messages, model);
            return fauxAssistantMessage("Done.");
        },
    ]);
    say(run.connection, "Make a file.");
    await vi.waitFor(() =>
        expect(
            changes(run.messages).find((change) => change.type === "wait"),
        ).toMatchObject({
            wait: {
                says: "wants to edit",
                about: "a.txt\nA new file:\none",
                change: { path: "a.txt", created: true, added: 1 },
                answers: ["allow", "always", "deny"],
            },
        }),
    );
    const wait = changes(run.messages).find((change) => change.type === "wait");
    if (wait?.type !== "wait") throw new Error("No wait");
    expect(second).toBeUndefined();
    for (const doc of ["charter", "character", "notes"] as const) {
        expect(
            await request(
                run.connection,
                run.messages,
                doc === "charter"
                    ? { type: "edit", id: randomUUID(), doc, text: "Be brief." }
                    : {
                          type: "edit",
                          id: randomUUID(),
                          doc,
                          agent: mayor,
                          text: doc === "character" ? "Calm." : "Tea.",
                      },
            ),
        ).toMatchObject({ kind: "notice" });
    }
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "limits",
            hamlet: "home",
            set: { section: "pinned", key: "mayor", value: "faux::faux-2" },
            reason: "More room. sk-chief-model-private",
        }),
    ).toMatchObject({ kind: "notice" });
    expect(
        await request(run.connection, run.messages, {
            type: "answer",
            id: randomUUID(),
            wait: wait.wait.id,
            choice: "dismiss",
        }),
    ).toMatchObject({
        type: "fault",
        text: expect.stringContaining("That cannot be answered that way:"),
    });
    expect(
        await request(run.connection, run.messages, {
            type: "answer",
            id: randomUUID(),
            wait: wait.wait.id,
            choice: "allow",
        }),
    ).toMatchObject({ kind: "notice" });
    await vi.waitFor(() =>
        expect(second).toMatchObject({
            model: "faux-2",
            sections: {
                charter: "Be brief.",
                character: "# Your character\nCalm.",
                notes: "# NOTES.md\nTea.",
            },
        }),
    );
    expect(
        (
            await run.home.harness.snapshot(RecordDoc, "home", context)
        )?.decisions.at(-1),
    ).toMatchObject({
        by: "chief",
        model: "faux::faux-2",
        reason: "More room. [REDACTED]",
    });
    expect(changes(run.messages)).toContainEqual({
        type: "wait-closed",
        id: wait.wait.id,
        how: "allow",
    });
});

it("stops a held turn by the founder's name, drops its wait and queue, and retries as a notice (C25, C26)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await setFolder(run.home, scratch());
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Carried on."),
    ]);
    await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    say(run.connection, "Wait.");
    await vi.waitFor(() =>
        expect(
            changes(run.messages).some((change) => change.type === "wait"),
        ).toBe(true),
    );
    expect(
        await request(run.connection, run.messages, {
            type: "stop",
            id: randomUUID(),
            agent: { hamlet: "home", agent: "founder" },
        }),
    ).toMatchObject({ kind: "notice", text: "Stopping mayor." });
    expect(changes(run.messages)).toContainEqual(
        expect.objectContaining({ type: "wait-closed", how: "stopped" }),
    );
    expect(
        await request(run.connection, run.messages, {
            type: "retry",
            id: randomUUID(),
            to: mayor,
        }),
    ).toMatchObject({ kind: "notice" });
    await vi.waitFor(() =>
        expect(agents(run.messages).at(-1)).toMatchObject({
            busy: false,
            saying: { text: "Carried on.", thought: false },
        }),
    );
    const readBack = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    expect(readBack).toMatchObject({
        type: "transcript",
        items: expect.arrayContaining([
            expect.objectContaining({
                kind: "notice",
                text: "Carry on from where you stopped.",
            }),
        ]),
    });
});

it("keeps the same conversations, entries and waits for typed and structured conversation deeds (S6)", async () => {
    const { rewind } = await import("../src/chats.ts");
    const { ChatDoc } = await import("../src/chats.ts");
    const { mayorChat, transcript, openWaits } = await import("./support.ts");
    const results = [];
    for (const structured of [false, true]) {
        const { models, faux } = fauxModels();
        const run = await served({
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const line = (text: string) =>
            request(run.connection, run.messages, {
                type: "say",
                id: randomUUID(),
                to: mayor,
                text,
            });
        const deed = (type: "new" | "resume" | "stop" | "retry") =>
            structured
                ? request(
                      run.connection,
                      run.messages,
                      type === "stop"
                          ? { type, id: randomUUID(), agent: mayor }
                          : type === "resume"
                            ? { type, id: randomUUID(), to: mayor, number: 1 }
                            : { type, id: randomUUID(), to: mayor },
                  )
                : line(type === "resume" ? "/resume 1" : `/${type}`);
        await deed("new");
        await deed("resume");
        await setFolder(run.home, scratch());
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("bash", { command: "echo held" }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Carried on."),
        ]);
        say(run.connection, "Held.");
        await vi.waitFor(async () =>
            expect(await openWaits(run.home)).toHaveLength(1),
        );
        await deed("stop");
        await deed("retry");
        const conversation = await mayorChat(run.home);
        await conversation.waitForIdle(context);
        const before = await transcript(conversation);
        const chief = before.find((entry) => entry.kind === "hamlet.chief");
        if (chief === undefined) throw new Error("No chief line");
        if (structured) {
            expect(
                await request(run.connection, run.messages, {
                    type: "rewind",
                    id: randomUUID(),
                    chat,
                    item: `${chief.id}:0`,
                }),
            ).toMatchObject({ kind: "rewound", text: "Held." });
        } else {
            expect(
                await rewind(
                    run.home,
                    {
                        agent: { hamlet: "home", number: 0 },
                        chief: run.home.chief,
                    },
                    `${chief.id}:0`,
                    context,
                ),
            ).toBe("Held.");
        }
        const index = (
            await run.home.harness.snapshot(ChatDoc, "home:0", context)
        )?.chats[run.home.chief];
        const after = await transcript(await mayorChat(run.home));
        results.push({
            current: index?.current,
            numbers: index?.conversations.map((each) => each.number),
            before: before.flatMap((entry) => {
                if (ChiefEntry.is(entry) || NoticeEntry.is(entry))
                    return [{ kind: entry.kind, text: entry.data.text }];
                if (RetryEntry.is(entry))
                    return [
                        {
                            kind: entry.kind,
                            text: String(entry.model?.[0]?.content),
                        },
                    ];
                return [];
            }),
            after: after
                .filter((entry) => entry.kind === "hamlet.chief")
                .map((entry) => entry.data),
            waits: await openWaits(run.home),
        });
    }
    expect(results[0]).toEqual({
        current: 1,
        numbers: [1, 2],
        before: [
            { kind: "hamlet.notice", text: "Went back to an earlier session." },
            { kind: "hamlet.chief", text: "Held." },
            { kind: "hamlet.notice", text: "Stopped: stopped by chief" },
            { kind: "hamlet.retry", text: "Carry on from where you stopped." },
        ],
        after: [],
        waits: [],
    });
    expect(results[0]).toEqual(results[1]);
});
