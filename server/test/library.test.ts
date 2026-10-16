import {
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it } from "vitest";
import { agentEnv } from "../src/environment.ts";
import { installStarters } from "../src/library.ts";
import {
    context,
    fauxModels,
    mayorChat,
    type Request,
    request,
    say,
    scratch,
    sh,
    testHome,
} from "./support.ts";

/** The starters the server ships. */
const starters = new URL("../assets/starters/", import.meta.url);

describe("the starters (starters.rs:120, :160)", () => {
    it("install once, keep what stands, edits and deletions, and bring untouched files up to date", async () => {
        const library = scratch();
        const weather = join(library, "weather");
        mkdirSync(weather);
        writeFileSync(join(weather, "SKILL.md"), "mine");

        const written = await installStarters(library);

        expect(written).toHaveLength(readdirSync(starters).length - 1);
        expect(written).not.toContain("weather");
        expect(readFileSync(join(weather, "SKILL.md"), "utf8")).toBe("mine");
        expect(
            existsSync(join(library, "web-search", "scripts", "search.py")),
        ).toBe(true);

        rmSync(join(library, "web-search"), { recursive: true });
        writeFileSync(join(library, "call-api", "SKILL.md"), "edited");
        // system-info's SKILL.md as an earlier version shipped it.
        const system = join(library, "system-info", "SKILL.md");
        const now = readFileSync(system, "utf8");
        const before = now
            .split("\n")
            .filter((line) => !line.startsWith("usage:"))
            .join("\n");
        writeFileSync(system, before);

        expect(await installStarters(library)).toEqual([]);
        expect(readFileSync(system, "utf8")).toBe(now);
        expect(existsSync(join(library, "web-search"))).toBe(false);
        expect(
            readFileSync(join(library, "call-api", "SKILL.md"), "utf8"),
        ).toBe("edited");
    });

    it("each have a front matter that names them and says what they do", () => {
        for (const name of readdirSync(starters)) {
            const skill = readFileSync(
                new URL(`${name}/SKILL.md`, starters),
                "utf8",
            );
            expect(skill.startsWith("---\n")).toBe(true);
            expect(skill).toContain(`name: ${name}\n`);
            expect(skill).toContain("description: ");
        }
    });
});

describe("the library (library.rs:361)", () => {
    it("is told in every prompt, each skill run from where it stands", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        let seen: Request | undefined;
        faux.setResponses([
            (transcript, _options, _state, model) => {
                seen = request(transcript.messages, model);
                return fauxAssistantMessage("Done.");
            },
        ]);

        await (await say(home, "Hello.")).wait(context);

        const library = join(dir, "library");
        expect(seen?.sections.library).toContain(
            `# Library\nSkills every agent shares, kept in ${library}.`,
        );
        expect(seen?.sections.library).toContain(
            `\n- web-search: Search the web and get the top results (title, URL, snippet) as plain text. Run: \`python3 ${library}/web-search/scripts/search.py "your query" [max_results]\` (${library}/web-search/SKILL.md)`,
        );
        expect(seen?.sections.library).toContain(
            `\n- manage-notes: How to keep NOTES.md short, current and useful as long-term memory. (${library}/manage-notes/SKILL.md)`,
        );
    });

    it("can be read by a command and never written", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const home = await testHome(dir, { models });
        const chat = await mayorChat(home);
        const env = await agentEnv(
            home,
            { conversationId: chat.id, read: home.harness },
            { net: false },
            context,
        );
        if (env === undefined) throw new Error("The mayor has no environment");
        const library = join(dir, "library");

        const read = await sh(env, `cat '${library}/web-search/SKILL.md'`);
        const write = await sh(env, `touch '${library}/web-search/mine'`);

        expect(read).toMatchObject({ exitCode: 0 });
        expect(read.output).toContain("name: web-search");
        expect(write.exitCode).not.toBe(0);
        expect(existsSync(join(library, "web-search", "mine"))).toBe(false);
    });
});
