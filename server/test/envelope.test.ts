import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { Type } from "typebox";
import { Value } from "typebox/value";
import { expect, it, vi } from "vitest";
import { readCommand } from "../src/commands.ts";
import {
    type ClientMessage,
    parseClient,
    parseServer,
    type Transcript,
    type Turn,
} from "../src/protocol.ts";
import { type ChiefResult, RealmDoc } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { err, ok } from "../src/result.ts";
import { faultFor, replyFor } from "../src/server.ts";
import { watchChat } from "../src/transcript.ts";
import { request, served } from "./connection-support.ts";
import {
    closeAfter,
    context,
    fauxModels,
    scratch,
    testHome,
} from "./support.ts";

it("correlates receipts and refusals and scrubs current credentials in every envelope field", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const id = randomUUID();
    await home.credentials.modify("test", async () => ({
        type: "api_key",
        key: "envelope-secret",
    }));
    expect(
        replyFor(
            id,
            ok({
                text: "Saved envelope-secret.",
                kind: "rewound",
                upload: "envelope-secret",
            }),
            home.secrets,
        ),
    ).toEqual({
        type: "reply",
        to: id,
        kind: "rewound",
        text: "Saved [REDACTED].",
        upload: "[REDACTED]",
    });
    const refusal = {
        kind: "hamlet-removed",
        hamlet: "orchard",
        operation: "say",
        retrySafe: true,
        reason: "envelope-secret",
        recovery: "restore-hamlet",
    } as const;
    expect(replyFor(id, err(refusal), home.secrets)).toEqual({
        type: "reply",
        to: id,
        kind: "error",
        text: "Cannot say: Hamlet `orchard` was removed: [REDACTED]. Ask a chief to restore it first.",
    });
    expect(faultFor(id, refusal, home.secrets)).toEqual({
        type: "fault",
        to: id,
        text: "Cannot say: Hamlet `orchard` was removed: [REDACTED]. Ask a chief to restore it first.",
    });
    expect(
        faultFor(
            undefined,
            {
                kind: "malformed",
                text: "There is no `envelope-secret` message.",
            },
            home.secrets,
        ),
    ).toEqual({
        type: "fault",
        text: "There is no `[REDACTED]` message.",
    });
});

it("keeps command usage wording and the historical notice or error policy behind the envelope", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const id = randomUUID();
    for (const [input, kind, text] of [
        ["/to", "error", "usage: /to <agent> <instruction>"],
        ["/revoke a", "notice", "usage: /revoke <agent> <network|unconfined>"],
        ["/reply", "notice", "say what to reply: /reply <text>"],
        [
            "/resume wrong",
            "notice",
            "Say which: `/resume <number>`, with the number History shows for a past session.",
        ],
    ]) {
        const command = readCommand(input ?? "");
        if (command === undefined || !("kind" in command))
            throw new Error("Expected a classified command failure");
        expect(replyFor(id, err(command), home.secrets)).toEqual({
            type: "reply",
            to: id,
            kind,
            text,
        });
    }
});

it("keeps returned refusals as replies and turns a thrown handler failure into a scrubbed correlated fault", async () => {
    const run = await served({ models: fauxModels().models });
    await run.home.credentials.modify("test", async () => ({
        type: "api_key",
        key: "missing-secret-agent",
    }));
    const diagnostic = vi.spyOn(console, "error").mockImplementation(() => {});
    closeAfter(() => diagnostic.mockRestore());
    const id = randomUUID();
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id,
            what: "papers",
            agent: { hamlet: "home", agent: "missing-secret-agent" },
        }),
    ).toEqual({
        type: "fault",
        to: id,
        text: "no agent named `[REDACTED]`",
    });
    expect(diagnostic).toHaveBeenCalledWith(expect.stringContaining(id));
    expect(JSON.stringify(diagnostic.mock.calls)).not.toContain(
        "missing-secret-agent",
    );
    const refused = randomUUID();
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: refused,
            to: { hamlet: "absent", agent: "mayor" },
            text: "hello",
        }),
    ).toEqual({
        type: "reply",
        to: refused,
        kind: "error",
        text: "Cannot say: no hamlet named `absent`. Choose a living hamlet.",
    });
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what: "charter",
        }),
    ).toMatchObject({ type: "read" });
});

const recordedFrame = Type.Object({
    from: Type.Enum(["client", "server"]),
    message: Type.Unknown(),
});

it("keeps the recorded refusal text bytes when legacy owner exceptions reach the fault path", async () => {
    const run = await served({ models: fauxModels().models });
    for (const name of ["reads-and-edits", "give"]) {
        const text = await readFile(
            new URL(`./frames/${name}.jsonl`, import.meta.url),
            "utf8",
        );
        const requests = new Map<string, ClientMessage>();
        for (const line of text.trimEnd().split("\n")) {
            const frame: unknown = JSON.parse(line);
            if (!Value.Check(recordedFrame, frame))
                throw new Error("Invalid recorded frame");
            if (frame.from === "client") {
                const parsed = parseClient(JSON.stringify(frame.message));
                if (!parsed.ok) throw new Error(parsed.error.text);
                if ("id" in parsed.value)
                    requests.set(parsed.value.id, parsed.value);
                continue;
            }
            const parsed = parseServer(JSON.stringify(frame.message));
            if (!parsed.ok) throw new Error(parsed.error.text);
            const message = parsed.value;
            if (message.type !== "reply" || message.kind !== "error") continue;
            const original = requests.get(message.to);
            if (original === undefined || !("id" in original))
                throw new Error("No recorded request");
            const id = randomUUID();
            expect(
                await request(run.connection, run.messages, {
                    ...original,
                    id,
                }),
            ).toEqual({
                type: "fault",
                to: id,
                text: message.text,
            });
        }
    }
});

it("lets the envelope render a watch's returned failure without a prebuilt fault", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const updates: ChiefResult<Transcript | Turn>[] = [];
    const outcome = await watchChat(
        home,
        { with: { hamlet: "home", agent: "mayor" } },
        (update) => updates.push(update),
        undefined,
        context,
    );
    if (!outcome.ok) throw new Error("Watch refused");
    closeAfter(() => outcome.value.stop());
    expect(updates[0]).toMatchObject({
        ok: true,
        value: { type: "transcript" },
    });
    await home.harness.commit(async (tx) => {
        const place = (await tx.doc(RealmDoc)).hamlets[0];
        if (!place) throw new Error("No hamlet");
        place.chiefs = ["another-chief"];
        const mayor = (await tx.doc(RecordDoc, "home", "home")).agents[0];
        if (!mayor) throw new Error("No mayor");
        mayor.purpose = "Tend the orchard";
    }, context);
    await vi.waitFor(() =>
        expect(updates.at(-1)).toMatchObject({
            ok: false,
            error: { kind: "not-chief" },
        }),
    );
    const failure = updates.at(-1);
    if (failure === undefined || failure.ok)
        throw new Error("No watch refusal");
    expect(faultFor(undefined, failure.error, home.secrets)).toEqual({
        type: "fault",
        text: "Cannot open chat: Only a chief of that hamlet may reach its agents. Ask one of its chiefs.",
    });
});
