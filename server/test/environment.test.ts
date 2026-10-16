import { mkdirSync, realpathSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it } from "vitest";
import { envFor, footing } from "../src/environment.ts";
import type { Home } from "../src/home.ts";
import {
    closeAfter,
    context,
    type Faux,
    fauxModels,
    mayorChat,
    say,
    scratch,
    setFolder,
    sh,
    testHome,
    transcript,
} from "./support.ts";

/** The output of the mayor's bash for `command`. */
async function bash(home: Home, faux: Faux, command: string): Promise<string> {
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Done."),
    ]);
    expect((await (await say(home, "Run it.")).wait(context)).status).toBe(
        "done",
    );
    const result = (await transcript(await mayorChat(home)))
        .filter((entry) => entry.kind === "pi.tool-result")
        .at(-1);
    const [message] = result?.model ?? [];
    return message?.role === "toolResult"
        ? message.content
              .flatMap((part) => (part.type === "text" ? [part.text] : []))
              .join("")
        : "";
}

/** The mayor's environment, with the network or without. */
async function mayorEnv(home: Home, net: boolean) {
    const conversation = await mayorChat(home);
    const env = await envFor(
        home,
        { conversationId: conversation.id, read: home.harness },
        { net },
        context,
    );
    if (env === undefined) throw new Error("the mayor has no environment");
    return env;
}

describe("the mayor's commands", () => {
    it("run in its house, and in the folder once the hamlet has one", async () => {
        const dir = scratch();
        const folder = scratch();
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });

        expect((await bash(home, faux, "pwd -P")).trim()).toBe(
            realpathSync(join(dir, "houses/home/0")),
        );
        await setFolder(home, folder, "auto");
        expect((await bash(home, faux, "pwd -P")).trim()).toBe(
            realpathSync(folder),
        );
    });

    it.each([
        ["no folder", false],
        ["a folder", true],
    ])(
        "with %s, write the folder and the house, read the library and the tools, and never read the rest of the home",
        async (_with, hasFolder) => {
            const dir = scratch();
            const folder = scratch();
            const { models } = fauxModels();
            const home = await testHome(dir, { models });
            if (hasFolder) await setFolder(home, folder, "auto");
            mkdirSync(join(dir, "library/forecast"));
            writeFileSync(join(dir, "library/forecast/SKILL.md"), "forecast");
            mkdirSync(join(dir, "tools/shout"));
            writeFileSync(join(dir, "tools/shout/manifest.json"), "{}");
            writeFileSync(join(dir, "models.json"), "{}");
            const env = await mayorEnv(home, false);
            const house = join(dir, "houses/home/0");
            const tries: { what: string; command: string; can: boolean }[] = [
                {
                    what: "write the house",
                    command: `echo x > '${house}/made'`,
                    can: true,
                },
            ];
            if (hasFolder) {
                tries.push({
                    what: "write the folder",
                    command: `echo x > '${folder}/made'`,
                    can: true,
                });
            }
            for (const file of [
                "library/forecast/SKILL.md",
                "tools/shout/manifest.json",
            ]) {
                const path = join(dir, file);
                tries.push(
                    {
                        what: `read ${file}`,
                        command: `cat '${path}'`,
                        can: true,
                    },
                    {
                        what: `write ${file}`,
                        command: `echo x >> '${path}'`,
                        can: false,
                    },
                );
            }
            for (const file of [
                "home.sqlite",
                "credentials.json",
                "models.json",
            ]) {
                tries.push({
                    what: `read ${file}`,
                    command: `cat '${join(dir, file)}'`,
                    can: false,
                });
            }

            // One sandboxed script for every try, each line saying how it went.
            const { output } = await sh(
                env,
                tries
                    .map(
                        ({ what, command }) =>
                            `if (${command}) >/dev/null 2>&1; then echo '${what}: yes'; else echo '${what}: no'; fi`,
                    )
                    .join("\n"),
            );

            expect(output.trim().split("\n")).toEqual(
                tries.map(({ what, can }) => `${what}: ${can ? "yes" : "no"}`),
            );
        },
    );

    it("reach the network only in an environment made with it", async () => {
        const server = createServer((_request, response) =>
            response.end("reached"),
        );
        await new Promise<void>((resolve) =>
            server.listen(0, "127.0.0.1", resolve),
        );
        closeAfter(() => server.close());
        const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models });

        expect(
            (await sh(await mayorEnv(home, false), `curl -sS -m 2 ${url}`))
                .output,
        ).not.toContain("reached");
        expect(
            (await sh(await mayorEnv(home, true), `curl -sS -m 2 ${url}`))
                .output,
        ).toBe("reached");
    });
});

describe("a tool's path", () => {
    const place = () => {
        const house = scratch();
        const folder = scratch();
        writeFileSync(join(house, "NOTES.md"), "# Notes\n");
        mkdirSync(join(house, "skills"));
        mkdirSync(join(house, "inbox"));
        mkdirSync(join(house, "src"));
        mkdirSync(join(folder, "src"));
        return { house, folder };
    };

    it("is the folder's when relative, unless only the house has its first part", () => {
        const { house, folder } = place();

        expect(footing("src/a.rs", { house, folder })).toBe(
            join(folder, "src/a.rs"),
        );
        expect(footing("README.md", { house, folder })).toBe(
            join(folder, "README.md"),
        );
        expect(footing("NOTES.md", { house, folder })).toBe(
            join(house, "NOTES.md"),
        );
        expect(footing("skills/new/SKILL.md", { house, folder })).toBe(
            join(house, "skills/new/SKILL.md"),
        );
        expect(footing("inbox/report.pdf", { house, folder })).toBe(
            join(house, "inbox/report.pdf"),
        );
    });

    it("stays where a full path puts it", () => {
        const { house, folder } = place();

        expect(footing(join(house, "own.txt"), { house, folder })).toBe(
            join(house, "own.txt"),
        );
        expect(footing(join(folder, "NOTES.md"), { house, folder })).toBe(
            join(folder, "NOTES.md"),
        );
    });

    it("is the house's when there is no folder, with no workspace/ alias", () => {
        const { house } = place();

        expect(footing("src/a.rs", { house, folder: null })).toBe(
            join(house, "src/a.rs"),
        );
        expect(footing("workspace/NOTES.md", { house, folder: null })).toBe(
            join(house, "workspace/NOTES.md"),
        );
    });
});
