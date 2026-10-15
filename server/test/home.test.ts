import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
    existsSync,
    mkdirSync,
    readFileSync,
    statSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { connect } from "node:net";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { HOME } from "../src/hamlet.ts";
import { openHome } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { addressed, addressOf, RecordDoc } from "../src/record.ts";
import { SettingsDoc, settingsOf } from "../src/settings.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

const founder = { number: 0, name: "founder" };

describe("opening a home", () => {
    it("lays out the mayor's house, the library and the tools folder, and records the founder", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const home = await testHome(dir, { models });

        const house = join(dir, "houses", "home", "0");
        expect(readFileSync(join(house, "NOTES.md"), "utf8")).toBe("# Notes\n");
        for (const folder of [
            join(house, "skills"),
            join(house, "inbox"),
            join(dir, "library"),
            join(dir, "tools"),
        ]) {
            expect(statSync(folder).isDirectory()).toBe(true);
        }
        const record = await home.harness.snapshot(RecordDoc, HOME, context);
        expect(record).toMatchObject({ agents: [founder], mayor: 0 });

        writeFileSync(join(house, "NOTES.md"), "# Notes\nThe chief.\n");
        await home.close(context);
        const again = await testHome(dir, { models });

        expect(readFileSync(join(house, "NOTES.md"), "utf8")).toBe(
            "# Notes\nThe chief.\n",
        );
        expect(await again.harness.snapshot(RecordDoc, HOME, context)).toEqual(
            record,
        );
    });

    it("gives the address mayor to whoever holds the office (H27)", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models });
        const to = (agent: string) =>
            addressed(home.harness, { hamlet: HOME, agent }, context);

        expect(await to("mayor")).toEqual({ hamlet: HOME, number: 0 });
        await home.harness.commit(async (tx) => {
            const record = await tx.doc(RecordDoc, HOME, HOME);
            record.agents.push({
                number: 1,
                name: "scout",
                purpose: "Looks around.",
                model: null,
                since: 1,
            });
            record.mayor = 1;
        }, context);

        expect(await to("mayor")).toEqual({ hamlet: HOME, number: 1 });
        expect(await to("founder")).toEqual({ hamlet: HOME, number: 0 });
        expect(await to("nobody")).toBeUndefined();
        const record = await home.harness.snapshot(RecordDoc, HOME, context);
        if (record === undefined) throw new Error("no record");
        expect([addressOf(record, 0), addressOf(record, 1)]).toEqual([
            "founder",
            "mayor",
        ]);
    });

    it("starts every shared document at Rust's defaults", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models, env: {} });

        expect(
            await home.harness.snapshot(LimitsDoc, HOME, context),
        ).toMatchObject({
            budget: { period: "day", cost: 5, minutes: null },
            task: { cost: 1, messages: 12, minutes: null },
            asking: { refusals: 3, questions: 3 },
            permissions: { network: [], unconfined: [] },
            mayor: {
                createAgents: true,
                buildRoads: true,
                removeAgents: true,
                mergeAgents: true,
            },
            pinned: {},
            model: null,
        });
        expect(await home.harness.snapshot(SettingsDoc, context)).toEqual({
            model: "local::qwen3.6:35b-a3b",
            reflectEvery: 10,
            allowNet: false,
            endpoints: [
                { name: "local", baseUrl: "http://localhost:11434/v1" },
            ],
            link: { state: "untested", models: null, words: null },
            saved: false,
        });
    });
});

describe("the environment's overrides", () => {
    it("take HAMLET_MODEL, HAMLET_REFLECT_EVERY and HAMLET_NET in memory and write none of them", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: {
                HAMLET_MODEL: "far::large",
                HAMLET_REFLECT_EVERY: "3",
                HAMLET_NET: "1",
            },
        });
        const stored = await home.harness.snapshot(SettingsDoc, context);

        expect(settingsOf(stored, home.overrides)).toMatchObject({
            model: "far::large",
            reflectEvery: 3,
            allowNet: true,
        });
        expect(stored).toMatchObject({
            model: "local::qwen3.6:35b-a3b",
            reflectEvery: 10,
            allowNet: false,
        });
    });

    it("move the endpoint that serves the default model with HAMLET_BASE_URL, in memory", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: {
                HAMLET_MODEL: "local::small",
                HAMLET_BASE_URL: "http://localhost:8080/v1",
            },
        });
        const stored = await home.harness.snapshot(SettingsDoc, context);

        expect(settingsOf(stored, home.overrides).endpoints).toEqual([
            { name: "local", baseUrl: "http://localhost:8080/v1" },
        ]);
        expect(stored?.endpoints).toEqual([
            { name: "local", baseUrl: "http://localhost:11434/v1" },
        ]);
        expect(
            settingsOf(stored, { baseUrl: "http://localhost:8080/v1" })
                .endpoints,
        ).toEqual([{ name: "local", baseUrl: "http://localhost:8080/v1" }]);
        // An endpoint that serves something else stays where it is.
        expect(
            settingsOf(stored, {
                model: "far::large",
                baseUrl: "http://localhost:8080/v1",
            }).endpoints,
        ).toEqual([{ name: "local", baseUrl: "http://localhost:11434/v1" }]);
    });

    it("fail the open on a HAMLET_REFLECT_EVERY that is no whole number, and leave the home free", async () => {
        const dir = scratch();
        const { models } = fauxModels();

        await expect(
            openHome(
                dir,
                { models, env: { HAMLET_REFLECT_EVERY: "x" } },
                context,
            ),
        ).rejects.toThrow(
            "HAMLET_REFLECT_EVERY must be a whole number, not `x`",
        );
        await testHome(dir, { models });
    });

    it.each([
        ["1", true],
        ["0", false],
        ["true", false],
        ["", false],
    ])("turn the network on for HAMLET_NET=%j: %s", async (value, on) => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_NET: value },
        });
        const stored = await home.harness.snapshot(SettingsDoc, context);
        expect(settingsOf(stored, home.overrides).allowNet).toBe(on);
    });
});

describe("the lock on a home", () => {
    it("refuses a second server while the first holds the home, and lets one in once it closed", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const first = await openHome(dir, { models }, context);

        await expect(openHome(dir, { models }, context)).rejects.toThrow(
            "A server already runs for",
        );
        await first.close(context);
        await testHome(dir, { models });
    });

    it.each(["trailing slash", "parent component", "symlink"])(
        "refuses a second spelling of the same home with a %s (S2)",
        async (spelling) => {
            const dir = scratch();
            const alias = join(scratch(), "home");
            symlinkSync(dir, alias);
            const other =
                spelling === "trailing slash"
                    ? `${dir}/`
                    : spelling === "parent component"
                      ? `${dir}/../${dir.split("/").at(-1)}`
                      : alias;
            const { models } = fauxModels();
            const first = await testHome(dir, { models });

            await expect(testHome(other, { models })).rejects.toThrow(
                "A server already runs for",
            );
            await first.close(context);
            await testHome(other, { models });
        },
    );

    it("claims the resolved storage folder when a path combines a symlink and `..` (S2)", async () => {
        const base = scratch();
        mkdirSync(join(base, "other/nested"), { recursive: true });
        mkdirSync(join(base, "other/home"));
        symlinkSync(join(base, "other/nested"), join(base, "alias"));
        const { models } = fauxModels();
        await testHome(join(base, "home"), { models });

        await expect(
            testHome(`${base}/alias/../home`, { models }),
        ).rejects.toThrow("A server already runs for");
    });

    it("takes over the lock a dead holder left behind", async () => {
        const dir = scratch();
        const lock = new URL("../src/lock.ts", import.meta.url).href;
        const holder = spawn(
            process.execPath,
            [
                "--input-type=module",
                "-e",
                `import { claim } from ${JSON.stringify(lock)};
                await claim(${JSON.stringify(dir)});
                console.log("held");
                setInterval(() => {}, 1000);`,
            ],
            { stdio: ["ignore", "pipe", "ignore"] },
        );
        await new Promise<void>((resolve) =>
            holder.stdout.once("data", () => resolve()),
        );
        const { models } = fauxModels();
        await expect(openHome(dir, { models }, context)).rejects.toThrow(
            "A server already runs for",
        );
        holder.kill("SIGKILL");
        await new Promise((resolve) => holder.once("exit", resolve));

        await testHome(dir, { models });
    });

    it("reads the Rust app's request to show its window, and ignores it", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const home = await testHome(dir, { models });

        await show(rustSocket(dir));

        await expect(openHome(dir, { models }, context)).rejects.toThrow(
            "A server already runs for",
        );
        expect(
            await home.harness.snapshot(RecordDoc, HOME, context),
        ).toBeDefined();
    });

    it("names its socket from the home as the Rust app spells it, keeping a trailing slash and dropping `.`", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        await testHome(`${dir}/./home/`, { models });

        // std::path::absolute in main.rs:107 keeps the trailing slash.
        await show(rustSocket(`${dir}/home/`));
    });

    it("opens a home whose path is too long for a socket in it", async () => {
        const dir = join(scratch(), "a".repeat(60), "b".repeat(60));
        const { models } = fauxModels();
        await testHome(dir, { models });

        await show(rustSocket(dir));
    });
});

/**
 * The socket the Rust app's instance.rs claims the home `dir` with: abstract on Linux, and elsewhere in interprocess's
 * special folder, `/run/user/<uid>/` where it exists and `/tmp/` otherwise.
 */
function rustSocket(dir: string): string {
    const tag = createHash("sha256").update(dir).digest("hex").slice(0, 12);
    const name = `hamlet-${tag}.sock`;
    if (process.platform === "linux") return `\0${name}`;
    const run = `/run/user/${process.getuid?.()}`;
    return existsSync(run) ? join(run, name) : join("/tmp", name);
}

/** Asks the holder of `socket` to show its window, as the Rust app does. */
function show(socket: string): Promise<void> {
    return new Promise<void>((resolve, reject) => {
        const stream = connect(socket, () => {
            stream.end("show\n");
        });
        stream.on("close", () => resolve());
        stream.on("error", reject);
    });
}
