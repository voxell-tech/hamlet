import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
    chmodSync,
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    symlinkSync,
    watch,
    writeFileSync,
} from "node:fs";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { houseOf, localDay } from "../src/home.ts";
import { LedgerDoc, reconcile } from "../src/ledger.ts";
import { LibraryDoc, libraryPart, libraryView } from "../src/library.ts";
import { LimitsDoc } from "../src/limits.ts";
import { request as sendRequest, served } from "./connection-support.ts";
import {
    context,
    crashing,
    fauxAgents,
    fauxModels,
    MAYOR,
    mayorChat,
    request,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

vi.setConfig({ testTimeout: 20000 });

function skill(
    house: string,
    name = "tides",
    paper = "---\nname: tides\ndescription: Tells the tide.\nusage: sh skills/tides/go.sh\n---\nRun it.",
) {
    const dir = join(house, "skills", name);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "SKILL.md"), paper);
    writeFileSync(join(dir, "go.sh"), "echo high\n");
    return dir;
}

it("moves an accepted skill to the shared shelf and records the librarian's reason (H33)", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const from = skill(houseOf(home.layout, MAYOR));
    writeFileSync(
        join(home.layout.librarian, "NOTES.md"),
        "Look for reusable commands.",
    );
    faux.setResponses([
        fauxAssistantMessage(
            [
                fauxToolCall("submit_skill", {
                    skill: "tides",
                    reason: "For ports.",
                }),
            ],
            { stopReason: "toolUse" },
        ),
        (prompt, _options, _state, model) => {
            const seen = request(prompt.messages, model);
            expect(seen.tools).toEqual([]);
            expect(seen.sections.instructions).toContain(
                "Look for reusable commands.",
            );
            expect(seen.sections.instructions).toContain(
                "free of anything private",
            );
            expect(JSON.stringify(prompt.messages)).toContain("go.sh");
            expect(JSON.stringify(prompt.messages)).toContain("web-search");
            return fauxAssistantMessage("ACCEPT. Clear and general.");
        },
        fauxAssistantMessage("Shared."),
    ]);
    expect(await (await say(home, "Share tides.")).wait(context)).toMatchObject(
        { status: "done" },
    );
    expect(existsSync(from)).toBe(false);
    expect(
        readFileSync(join(home.layout.library, "tides", "go.sh"), "utf8"),
    ).toBe("echo high\n");
    expect(await libraryPart(home.layout.library)).toContain(
        `sh ${home.layout.library}/tides/go.sh`,
    );
    expect(await libraryPart(home.layout.library)).toContain(
        "To share a skill of your own, use `submit_skill`.",
    );
    expect((await home.harness.snapshot(LibraryDoc, context))?.events).toEqual([
        {
            kind: "shelved",
            skill: "tides",
            by: "founder@home",
            reason: "Clear and general.",
            at: expect.any(Number),
        },
    ]);
    expect(await libraryView(home, context)).toContainEqual({
        name: "tides",
        description: "Tells the tide.",
        by: "founder@home",
    });
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "The librarian took",
    );
});

it("keeps a refused skill in its house with the librarian's words", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const from = skill(houseOf(home.layout, MAYOR));
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("I would accept it if it were general."),
        fauxAssistantMessage("Kept."),
    ]);
    await (await say(home, "Offer it.")).wait(context);
    expect(existsSync(from)).toBe(true);
    expect(existsSync(join(home.layout.library, "tides"))).toBe(false);
    expect(
        (await home.harness.snapshot(LibraryDoc, context))?.events,
    ).toMatchObject([
        {
            kind: "refused",
            by: "founder@home",
            reason: "would accept it if it were general.",
        },
    ]);
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "The librarian did not take",
    );
});

it.each(["bare", "duplicate", "link", "fifo", "name", "reason", "missing"])(
    "refuses an unfit or unsafe offer (%s) without asking a model",
    async (kind) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        const from = skill(
            houseOf(home.layout, MAYOR),
            kind === "duplicate" ? "weather" : "tides",
            kind === "bare" ? "---\nname: tides\n---" : undefined,
        );
        if (kind === "link")
            symlinkSync(home.layout.credentials, join(from, "secret"));
        if (kind === "fifo") execFileSync("mkfifo", [join(from, "pipe")]);
        let requests = 0;
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("submit_skill", {
                    skill:
                        kind === "name"
                            ? "../tides"
                            : kind === "duplicate"
                              ? "weather"
                              : kind === "missing"
                                ? "absent"
                                : "tides",
                    reason: kind === "reason" ? " " : "For ports.",
                }),
                { stopReason: "toolUse" },
            ),
            () => {
                requests++;
                return fauxAssistantMessage("Not offered.");
            },
        ]);
        await (await say(home, "Offer it.")).wait(context);
        expect(requests).toBe(1);
        expect(existsSync(from)).toBe(true);
        expect(existsSync(join(home.layout.library, "tides"))).toBe(false);
        const log = JSON.stringify(await transcript(await mayorChat(home)));
        expect(log).toContain(
            kind === "bare"
                ? "name and a description"
                : kind === "duplicate"
                  ? "already has"
                  : kind === "link" || kind === "fifo"
                    ? "links or special files"
                    : kind === "name"
                      ? "its folder under skills/"
                      : kind === "reason"
                        ? "who else would use it"
                        : "could not be delivered",
        );
    },
);

it("buys no librarian call once the budget has been spent", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const from = skill(houseOf(home.layout, MAYOR));
    let calls = 0;
    faux.setResponses([
        async () => {
            calls++;
            await home.harness.commit(async (tx) => {
                (await tx.doc(LimitsDoc, "home", "home")).budget.cost = 0;
            }, context);
            return fauxAssistantMessage(
                fauxToolCall("submit_skill", {
                    skill: "tides",
                    reason: "For ports.",
                }),
                { stopReason: "toolUse" },
            );
        },
    ]);
    await (await say(home, "Offer it.")).wait(context);
    expect(calls).toBe(1);
    expect(existsSync(from)).toBe(true);
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "spent its budget",
    );
});

it("answers shelf questions with an errand charged to the asker's hamlet and task", async () => {
    const { models, faux } = fauxModels(["faux-1"], 1);
    const home = await testHome(scratch(), { models });
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("ask_librarian", { question: "How can I search?" }),
            { stopReason: "toolUse" },
        ),
        (prompt, _options, _state, model) => {
            const seen = request(prompt.messages, model);
            expect(seen.tools).toEqual([]);
            expect(seen.sections.instructions).toContain(
                `python3 ${home.layout.library}/web-search/scripts/search.py`,
            );
            expect(seen.sections.instructions).toContain(
                "Do not invent a skill",
            );
            return fauxAssistantMessage("Use web-search.");
        },
        fauxAssistantMessage("Found it."),
    ]);
    const submitted = await say(home, "Find a skill.");
    await submitted.wait(context);
    await reconcile(home, context);
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "Use web-search.",
    );
    const day = await home.harness.snapshot(
        LedgerDoc,
        `home/${localDay(home.now())}`,
        context,
    );
    expect(day?.lines).toHaveLength(3);
    expect(
        day?.lines.every(
            (line) =>
                line.what !== "reset" &&
                line.agent === 0 &&
                line.task ===
                    (day.lines[0]?.what !== "reset"
                        ? day.lines[0]?.task
                        : null) &&
                line.micros > 0,
        ),
    ).toBe(true);
});

it("unshelves a starter over the protocol and never installs it again", async () => {
    const { models } = fauxModels();
    const { home, connection, messages, server } = await served({ models });
    const reply = await sendRequest(connection, messages, {
        type: "say",
        id: randomUUID(),
        to: { hamlet: "home", agent: "mayor" },
        text: "/unshelve web-search No longer needed.",
    });
    expect(reply).toMatchObject({ type: "reply", kind: "notice" });
    expect(existsSync(join(home.layout.library, "web-search"))).toBe(false);
    expect(
        (await home.harness.snapshot(LibraryDoc, context))?.events,
    ).toMatchObject([
        {
            kind: "unshelved",
            by: "chief:chief",
            skill: "web-search",
            reason: "No longer needed.",
        },
    ]);
    connection.close();
    await server.close();
    await home.close(context);
    const reopened = await testHome(home.dir, { models });
    expect(
        (await libraryView(reopened, context)).some(
            (s) => s.name === "web-search",
        ),
    ).toBe(false);
    expect(await libraryView(reopened, context)).toContainEqual(
        expect.objectContaining({ name: "weather", by: "Hamlet" }),
    );
});

it("finishes a shelved skill once after a crash before its record commits (S3)", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    const before = await testHome(dir, {
        models,
        openStorage: crashing((writes) =>
            writes.some(
                (w) =>
                    w.type === "document.create" &&
                    w.record.kind === "hamlet.library",
            ),
        ),
    });
    skill(houseOf(before.layout, MAYOR));
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("ACCEPT: Reusable."),
        fauxAssistantMessage("Shared."),
    ]);
    const submission = await say(before, "Share it.");
    expect(await before.harness.closed).toMatchObject({ reason: "failed" });
    const after = await testHome(dir, { models });
    expect(
        await (await after.harness.submission(submission.id, context))?.wait(
            context,
        ),
    ).toMatchObject({ status: "done" });
    expect(
        (await after.harness.snapshot(LibraryDoc, context))?.events,
    ).toMatchObject([{ kind: "shelved", skill: "tides" }]);
    expect(existsSync(join(houseOf(after.layout, MAYOR), "skills/tides"))).toBe(
        false,
    );
    expect(
        readFileSync(join(after.layout.library, "tides/go.sh"), "utf8"),
    ).toBe("echo high\n");
});

it("releases an offer whose librarian call failed so it can be offered again", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    skill(houseOf(home.layout, MAYOR));
    const offer = () =>
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        );
    faux.setResponses([
        offer(),
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Bad request",
        }),
        fauxAssistantMessage("Try later."),
        offer(),
        fauxAssistantMessage("ACCEPT: Reusable."),
        fauxAssistantMessage("Shared."),
    ]);
    await (await say(home, "Offer it.")).wait(context);
    await (await say(home, "Try again.")).wait(context);
    expect(existsSync(join(home.layout.library, "tides"))).toBe(true);
});

it("lets an agent offer its skill again after its earlier offer was stopped", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    skill(houseOf(home.layout, MAYOR));
    const offer = () =>
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        );
    let release = () => {};
    const released = new Promise<void>((resolve) => {
        release = resolve;
    });
    let reading = false;
    fauxAgents(faux, {
        "home:0": [offer(), offer(), fauxAssistantMessage("Shared.")],
        errands: [
            async () => {
                reading = true;
                await released;
                return fauxAssistantMessage("REFUSE: Later.");
            },
            fauxAssistantMessage("ACCEPT: Reusable."),
        ],
    });
    try {
        await say(home, "Offer it.");
        await vi.waitFor(() => expect(reading).toBe(true), { timeout: 10000 });
        await (await mayorChat(home)).abort(context);
        await (await say(home, "Try again.")).wait(context);
        expect(existsSync(join(home.layout.library, "tides"))).toBe(true);
    } finally {
        release();
    }
});

it("shelves the judged copy but keeps changes made in the house while it was judged", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const from = skill(houseOf(home.layout, MAYOR));
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        ),
        () => {
            writeFileSync(join(from, "go.sh"), "echo improved\n");
            writeFileSync(join(from, "tables.csv"), "later data\n");
            return fauxAssistantMessage("ACCEPT. Clear and general.");
        },
        fauxAssistantMessage("Shared."),
    ]);
    await (await say(home, "Offer")).wait(context);
    expect(existsSync(join(from, "tables.csv"))).toBe(true);
    expect(readFileSync(join(from, "go.sh"), "utf8")).toBe("echo improved\n");
    expect(readFileSync(join(home.layout.library, "tides/go.sh"), "utf8")).toBe(
        "echo high\n",
    );
    expect(JSON.stringify(await transcript(await mayorChat(home)))).toContain(
        "later changes stayed in your house",
    );
    expect(
        (await home.harness.snapshot(LibraryDoc, context))?.events,
    ).toMatchObject([{ kind: "shelved", by: "founder@home" }]);
});

it("records shelving even when a read-only house copy cannot be removed, without half deleting it", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const from = skill(houseOf(home.layout, MAYOR));
    mkdirSync(join(from, "bin"));
    writeFileSync(join(from, "bin/x"), "kept");
    chmodSync(join(from, "bin"), 0o555);
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("ACCEPT. Clear and general."),
        fauxAssistantMessage("Shared."),
    ]);
    try {
        await (await say(home, "Offer")).wait(context);
        expect(
            (await home.harness.snapshot(LibraryDoc, context))?.events,
        ).toMatchObject([{ kind: "shelved", by: "founder@home" }]);
        expect(await libraryView(home, context)).toContainEqual(
            expect.objectContaining({ name: "tides", by: "founder@home" }),
        );
        expect(existsSync(join(from, "SKILL.md"))).toBe(true);
        expect(existsSync(join(from, "bin/x"))).toBe(true);
        expect(
            JSON.stringify(await transcript(await mayorChat(home))),
        ).toContain("shelved, but its house copy could not be removed");
    } finally {
        chmodSync(join(from, "bin"), 0o755);
        if (existsSync(join(home.layout.library, "tides/bin")))
            chmodSync(join(home.layout.library, "tides/bin"), 0o755);
    }
});

it("publishes a whole skill atomically, never a partly copied shelf", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const from = skill(houseOf(home.layout, MAYOR));
    for (let i = 0; i < 512; i++)
        writeFileSync(join(from, `part-${i}`), "x".repeat(16384));
    const published = Promise.withResolvers<number>();
    const watcher = watch(home.layout.library, (_event, name) => {
        if (name === "tides" && existsSync(join(home.layout.library, "tides")))
            published.resolve(
                readdirSync(join(home.layout.library, "tides")).length,
            );
    });
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("ACCEPT. Clear and general."),
        fauxAssistantMessage("Shared."),
    ]);
    try {
        await (await say(home, "Offer")).wait(context);
        expect(await published.promise).toBe(514);
        expect(
            readdirSync(home.layout.library).filter((n) =>
                n.startsWith(".incoming-"),
            ),
        ).toEqual([]);
    } finally {
        watcher.close();
    }
});

it("sweeps an unclaimed library stage when the home opens", async () => {
    const { models } = fauxModels();
    const dir = scratch();
    const before = await testHome(dir, {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await before.close(context);
    const orphan = join(dir, ".library-stages", "offer-orphan");
    mkdirSync(orphan, { recursive: true });
    writeFileSync(join(orphan, "leftover"), "interrupted before its record");
    const incoming = join(dir, "library", ".incoming-interrupted", "tides");
    mkdirSync(incoming, { recursive: true });
    writeFileSync(join(incoming, "half-copied"), "partial");
    await testHome(dir, { models });
    expect(existsSync(orphan)).toBe(false);
    expect(existsSync(incoming)).toBe(false);
});

it.each([
    ["**ACCEPT** Clear and general.", "ACCEPT** Clear and general."],
    ["- ACCEPT: Clear.", "ACCEPT: Clear."],
    ["1. ACCEPT Clear.", "1. ACCEPT Clear."],
    ["accept. Clear.", "Clear."],
])(
    "takes a formatted first-word acceptance like Rust (%s)",
    async (verdict, reason) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        const from = skill(houseOf(home.layout, MAYOR));
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("submit_skill", {
                    skill: "tides",
                    reason: "For ports.",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage(verdict),
            fauxAssistantMessage("Shared."),
        ]);
        await (await say(home, "Offer")).wait(context);
        expect(existsSync(from)).toBe(false);
        expect(
            (await home.harness.snapshot(LibraryDoc, context))?.events,
        ).toMatchObject([{ kind: "shelved", reason }]);
    },
);

it("cleans an interrupted hidden delivery even after the caller is cancelled", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const from = skill(houseOf(home.layout, MAYOR));
    for (let i = 0; i < 1024; i++)
        writeFileSync(join(from, `part-${i}`), "x".repeat(16384));
    const chat = await mayorChat(home);
    const stopped = Promise.withResolvers<void>();
    let stopping = false;
    const watcher = watch(home.layout.library, (_event, name) => {
        if (!stopping && name?.startsWith(".incoming-")) {
            stopping = true;
            void chat
                .abort(context)
                .then(() => stopped.resolve(), stopped.reject);
        }
    });
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("submit_skill", {
                skill: "tides",
                reason: "For ports.",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("ACCEPT. Clear and general."),
        fauxAssistantMessage("Shared."),
    ]);
    try {
        const sub = await say(home, "Offer");
        await stopped.promise;
        await sub.wait(context);
        await vi.waitFor(() =>
            expect(
                readdirSync(home.layout.library).filter((n) =>
                    n.startsWith(".incoming-"),
                ),
            ).toEqual([]),
        );
        if (existsSync(join(home.layout.library, "tides"))) {
            expect(
                readdirSync(join(home.layout.library, "tides")),
            ).toHaveLength(1026);
            expect(
                (await home.harness.snapshot(LibraryDoc, context))?.events,
            ).toMatchObject([{ kind: "shelved" }]);
        } else {
            expect(existsSync(join(from, "SKILL.md"))).toBe(true);
        }
    } finally {
        watcher.close();
    }
});
