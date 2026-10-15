import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { getSystemMessageText } from "@earendil-works/pi-ai";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { defineExtension, section } from "@earendil-works/pi-durable";
import { describe, expect, it, vi } from "vitest";
import { CharacterDoc } from "../src/character.ts";
import { CharterDoc, DEFAULT_CHARTER, editCharter } from "../src/charter.ts";
import { commitEdit } from "../src/choice.ts";
import { HOME } from "../src/hamlet.ts";
import type { Home } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { ManifestsDoc } from "../src/manifests.ts";
import type { Part } from "../src/parts.ts";
import { told } from "../src/told.ts";
import {
    CHIEF,
    context,
    type Faux,
    fauxModels,
    mayorChat,
    type Request,
    request,
    say,
    scratch,
    setFolder,
    testHome,
} from "./support.ts";

/** The headings each part of the prompt opens with, in C11's order. */
const PARTS = [
    ["charter", DEFAULT_CHARTER.slice(0, 40)],
    ["machine", "# Environment\n"],
    ["project", "# The project\n"],
    ["character", "# Your character\n"],
    ["notes", "# NOTES.md\n"],
    ["skills", "# Skills\n"],
    ["library", "# Library\n"],
    ["budget", "# Budget\n"],
] as const;

/** The network sentence of a mayor without the switch (worker.rs:43-46). */
const HELD =
    "off unless a chief allows it; a command that needs it is held until the chief answers, so set `network` to true on it and carry on";

/** The OS line of this machine (prompt.rs:10-17). */
const SYSTEM =
    process.platform === "darwin"
        ? "System: macOS, whose shell tools are the BSD ones. Flags that only GNU has, such as `grep -P`, `cat -A` and `sed -r`, fail."
        : "System: Linux.";

/** The sections the mayor's next request sends, which it answers with "Done." */
async function sent(home: Home, faux: Faux): Promise<Record<string, string>> {
    let seen: Request | undefined;
    faux.setResponses([
        (transcript, _options, _state, model) => {
            seen = request(transcript.messages, model);
            return fauxAssistantMessage("Done.");
        },
    ]);
    expect((await (await say(home, "Hello.")).wait(context)).status).toBe(
        "done",
    );
    if (seen === undefined) throw new Error("No request was sent");
    return seen.sections;
}

/** A home whose mayor runs faux::faux-1, opened in a fresh folder. */
async function mayorHome(env: Record<string, string> = {}) {
    const dir = scratch();
    const { models, faux } = fauxModels();
    const home = await testHome(dir, { models, env });
    return { home, faux, house: join(dir, "houses", "home", "0") };
}

async function character(home: Home, text: string): Promise<void> {
    await home.harness.commit(async (tx) => {
        (await tx.doc(CharacterDoc, "home:0", "home:0")).text = text;
    }, context);
}

describe("the prompt", () => {
    it("opens the home and reports a library that cannot install starters (H33)", async () => {
        const dir = scratch();
        mkdirSync(join(dir, "library", ".starters"), { recursive: true });
        const { models, faux } = fauxModels();
        const err = vi
            .spyOn(process.stderr, "write")
            .mockImplementation(() => true);
        try {
            const home = await testHome(dir, { models });
            expect(err).toHaveBeenCalledWith(
                expect.stringMatching(
                    /^hamlet: could not open the library: .*\n$/,
                ),
            );
            expect((await sent(home, faux)).notes).toBe("# NOTES.md\n# Notes");
        } finally {
            err.mockRestore();
        }
    });

    it("sends C11's parts in order, under Rust's headings and with no tags", async () => {
        const { home, faux } = await mayorHome();
        await setFolder(home, scratch(), "auto");
        await character(home, "Speak plainly.");

        const sections = await sent(home, faux);

        expect(Object.keys(sections)).toEqual(PARTS.map(([key]) => key));
        for (const [key, opening] of PARTS) {
            expect(sections[key]?.startsWith(opening)).toBe(true);
        }
    });

    it("tells the machine: the sandbox, the network, the model, the OS and the workspace", async () => {
        const { home, faux, house } = await mayorHome();
        const { home: on, faux: onFaux } = await mayorHome({ HAMLET_NET: "1" });

        expect((await sent(home, faux)).machine).toBe(
            `# Environment\nSandbox: ${home.sandbox.name}. Network for tools: ${HELD}. Model: faux::faux-1.\n${SYSTEM}\nWorkspace: ${house}`,
        );
        expect((await sent(on, onFaux)).machine).toContain(
            "Network for tools: on. Model: faux::faux-1.\n",
        );
    });

    it("names the tools that failed to load (C15)", async () => {
        const { home, faux } = await mayorHome();
        await home.harness.commit(async (tx) => {
            (await tx.doc(ManifestsDoc)).faults = [
                "tools/a: missing name",
                "tools/b: name 'bash' is reserved",
            ];
        }, context);

        expect((await sent(home, faux)).machine).toContain(
            ". Model: faux::faux-1.\nTools that failed to load:\ntools/a: missing name\ntools/b: name 'bash' is reserved\nSystem: ",
        );
    });

    it("tells a project with its own instructions (prompt.rs:228)", async () => {
        const { home, faux, house } = await mayorHome();
        const folder = scratch();
        mkdirSync(join(house, "skills", "forecast"), { recursive: true });
        writeFileSync(
            join(house, "skills", "forecast", "SKILL.md"),
            "---\nname: forecast\nusage: sh get.sh\n---\n",
        );
        await setFolder(home, folder, "auto");

        let sections = await sent(home, faux);
        expect(sections.project).toContain(
            `# The project\nYour hamlet works on the folder ${folder}.`,
        );
        expect(sections.skills).toContain(
            `- forecast:  Run: \`cd '${house}' && sh get.sh\` (skills/forecast/SKILL.md)`,
        );
        expect(sections.project).not.toContain("git worktree");
        expect(sections.project).not.toContain("The chief approves each edit");
        expect(sections.project).not.toContain("# The project's");

        mkdirSync(join(folder, ".git"));
        writeFileSync(join(folder, "CLAUDE.md"), "Use tabs.");
        await setFolder(home, folder, "manual");
        sections = await sent(home, faux);
        expect(sections.project).toContain("git worktree");
        expect(sections.project).not.toContain("cannot be written");
        expect(sections.project).toContain("The chief approves each edit");
        expect(sections.project).toContain(
            "# The project's CLAUDE.md\nUse tabs.",
        );

        writeFileSync(join(folder, "AGENTS.md"), "y".repeat(8001));
        sections = await sent(home, faux);
        expect(sections.project).toContain(
            `# The project's AGENTS.md\n${"y".repeat(8000)}\n[AGENTS.md is longer than 8000 characters and is cut here.]`,
        );
        expect(sections.project).not.toContain("Use tabs.");
    });

    it("puts the character between the charter and the notes, and cuts it past 4000 characters (prompt.rs:278)", async () => {
        const { home, faux } = await mayorHome();

        expect((await sent(home, faux)).character).toBeUndefined();
        await character(home, " \n");
        expect((await sent(home, faux)).character).toBeUndefined();
        await character(home, "Speak plainly.\n");
        const sections = await sent(home, faux);
        const keys = Object.keys(sections);
        expect(sections.character).toBe("# Your character\nSpeak plainly.");
        expect(keys.indexOf("charter")).toBeLessThan(keys.indexOf("character"));
        expect(keys.indexOf("character")).toBeLessThan(keys.indexOf("notes"));
        await character(home, "\u{1f33b}".repeat(4001));
        expect((await sent(home, faux)).character).toBe(
            `# Your character\n${"\u{1f33b}".repeat(4000)}\n[The character is longer than 4000 characters and is cut here.]`,
        );
    });

    it("cuts NOTES.md past 8000 characters, counting characters, and shows the heading alone when it cannot be read", async () => {
        const { home, faux, house } = await mayorHome();
        const notes = join(house, "NOTES.md");
        writeFileSync(notes, "\u{1f33b}".repeat(8001));

        expect((await sent(home, faux)).notes).toBe(
            `# NOTES.md\n${"\u{1f33b}".repeat(8000)}\n[NOTES.md is longer than 8000 characters and is cut here. Shorten it so that all of it is shown.]`,
        );
        rmSync(notes);
        expect((await sent(home, faux)).notes).toBe("# NOTES.md");
        writeFileSync(notes, Buffer.from([0xff, 0xfe, 0x41]));
        expect((await sent(home, faux)).notes).toBe("# NOTES.md");
    });

    it("lists the house's skills with descriptions and commands that start in its house", async () => {
        const { home, faux, house } = await mayorHome();
        const skills = join(house, "skills");

        expect((await sent(home, faux)).skills).toBe("# Skills\n(none yet)");
        mkdirSync(join(skills, "forecast"), { recursive: true });
        writeFileSync(
            join(skills, "forecast", "SKILL.md"),
            '---\nname: forecast\ndescription: "Get the forecast"\nusage: sh skills/forecast/scripts/get.sh\n---\nRun scripts/get.sh\n',
        );
        mkdirSync(join(skills, "broken"));
        writeFileSync(join(skills, "broken", "SKILL.md"), "no front matter");

        expect((await sent(home, faux)).skills).toBe(
            `# Skills\n- broken:  (skills/broken/SKILL.md)\n- forecast: Get the forecast Run: \`cd '${house}' && sh skills/forecast/scripts/get.sh\` (skills/forecast/SKILL.md)`,
        );
    });

    it("keeps the default charter while the home has none, and saving the default stores none (charter.rs:52)", async () => {
        const { home, faux } = await mayorHome();
        const own = async () =>
            (await home.harness.snapshot(CharterDoc, context))?.own;

        expect((await sent(home, faux)).charter).toBe(
            DEFAULT_CHARTER.trimEnd(),
        );
        await editCharter(home.harness, "  \n", context);
        expect(await own()).toBeNull();
        await editCharter(home.harness, "Be kind.\n", context);
        expect((await sent(home, faux)).charter).toBe("Be kind.");
        await editCharter(home.harness, DEFAULT_CHARTER, context);
        expect(await own()).toBeNull();
        await editCharter(home.harness, "Be kind.", context);
        await editCharter(home.harness, null, context);
        expect((await sent(home, faux)).charter).toBe(
            DEFAULT_CHARTER.trimEnd(),
        );
    });

    it("names the server's own tools in the default charter", () => {
        for (const tool of [
            "read",
            "write",
            "edit",
            "bash",
            "search",
            "find",
        ]) {
            expect(DEFAULT_CHARTER).toContain(`\`${tool}\``);
        }
    });

    it("takes a chief's changes to the charter, the character and the notes from the agent's next request in the same run", async () => {
        const { models, faux } = fauxModels(["faux-1", "faux-2"]);
        const dir = scratch();
        const home = await testHome(dir, { models });
        const chat = await mayorChat(home);
        const read = (await chat.agent(context)).tools.find(
            (tool) => tool.name === "read",
        );
        const seen: Request[] = [];
        faux.setResponses([
            async (transcript, _options, _state, model) => {
                seen.push(request(transcript.messages, model));
                await editCharter(home.harness, "Be brief.", context);
                await character(home, "Calm.");
                writeFileSync(
                    join(dir, "houses", "home", "0", "NOTES.md"),
                    "# Notes\nThe chief likes tea.\n",
                );
                await home.harness.commit(
                    (tx) =>
                        commitEdit(home, tx, {
                            by: CHIEF,
                            change: async (tx) => {
                                (
                                    await tx.doc(LimitsDoc, HOME, HOME)
                                ).pinned.mayor = "faux::faux-2";
                            },
                        }),
                    context,
                );
                await chat.configure(
                    { tools: read === undefined ? [] : [read] },
                    context,
                );
                return fauxAssistantMessage(
                    fauxToolCall("read", { path: "NOTES.md" }),
                    { stopReason: "toolUse" },
                );
            },
            (transcript, _options, _state, model) => {
                seen.push(request(transcript.messages, model));
                return fauxAssistantMessage("Done.");
            },
        ]);

        const submission = await say(home, "Look at your notes.");
        expect((await submission.wait(context)).status).toBe("done");

        const [first, second] = seen;
        expect(first?.model).toBe("faux-1");
        expect(first?.sections.charter).toBe(DEFAULT_CHARTER.trimEnd());
        expect(first?.sections.character).toBeUndefined();
        expect(first?.tools).toEqual(
            expect.arrayContaining(["read", "edit", "bash"]),
        );
        expect(second?.model).toBe("faux-2");
        expect(second?.sections.charter).toBe("Be brief.");
        expect(second?.sections.character).toBe("# Your character\nCalm.");
        expect(second?.sections.notes).toBe(
            "# NOTES.md\n# Notes\nThe chief likes tea.",
        );
        expect(second?.tools).toEqual(["read"]);
    });

    it("keeps a failing section's previously shown text for the chief, without adding tags twice (C14)", async () => {
        let failing = false;
        const flaky: Part = () => ({
            extension: defineExtension({
                name: "test.flaky",
                sections: [
                    section("flaky", () => {
                        if (failing) throw new Error("cannot render");
                        return "Keep these words.";
                    }),
                    section("unshown", () => {
                        throw new Error("never rendered");
                    }),
                ],
            }),
        });
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models, parts: [flaky] });
        await sent(home, faux);
        failing = true;
        const shown = await told(home, await mayorChat(home), context);

        expect(shown).toContain("<flaky>\nKeep these words.\n</flaky>");
        expect(shown).not.toContain("<flaky>\n<flaky>");
        expect(shown).not.toContain("unshown");
        const next = await sent(home, faux);
        expect(shown).toBe(
            getSystemMessageText({
                role: "system",
                content: "",
                sections: next,
                timestamp: 0,
            }),
        );
    });

    it("shows a chief the prompt exactly as the next request sends it, a section that keeps its words through a run included (C14)", async () => {
        let renders = 0;
        // Its words stay as they were first sent, as the budget's do in a run.
        const steady: Part = () => ({
            extension: defineExtension({
                name: "test.steady",
                sections: [
                    section(
                        "steady",
                        (input) => {
                            renders += 1;
                            return input.shown.steady ?? `Render ${renders}.`;
                        },
                        { tag: false },
                    ),
                ],
            }),
        });
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models, parts: [steady] });
        await sent(home, faux);

        const shown = await told(home, await mayorChat(home), context);
        const next = await sent(home, faux);

        expect(shown).toBe(Object.values(next).join("\n\n"));
        expect(shown).toContain("Render 1.");
        expect(renders).toBeGreaterThan(2);
    });
});
