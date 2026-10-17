import { execFile } from "node:child_process";
import {
    chmodSync,
    copyFileSync,
    existsSync,
    mkdirSync,
    readFileSync,
    rmSync,
    statSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { describe, expect, it, vi } from "vitest";
import { connect } from "../src/connection.ts";
import { hamlet, type Run, server } from "./cli-support.ts";
import { joined, say } from "./connection-support.ts";
import { closeAfter, scratch } from "./support.ts";

function usage(text: string): void {
    for (const command of [
        "server",
        "chat",
        "dev",
        "window",
        "edit",
        "login",
        "models",
    ]) {
        expect(text).toContain(`hamlet ${command}`);
    }
    expect(text).not.toMatch(/--headless|--repl/);
}

type Capture = {
    server: string;
    token: string;
    view: string;
    argv: string[];
};

function fakeApp(hold = false): {
    app: string;
    env: NodeJS.ProcessEnv;
    captured(): Capture;
    terminated(): string;
} {
    const dir = scratch();
    const app = join(dir, "hamlet-app");
    const capture = join(dir, "capture.json");
    const terminated = join(dir, "terminated.txt");
    const pid = join(dir, "pid");
    closeAfter(() => {
        if (!hold || existsSync(terminated) || !existsSync(pid)) return;
        try {
            process.kill(Number(readFileSync(pid, "utf8")), "SIGKILL");
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
        }
    });
    writeFileSync(
        app,
        `#!${process.execPath}
const { writeFileSync } = require("node:fs");
writeFileSync(process.env.APP_CAPTURE, JSON.stringify({
    server: process.env.HAMLET_SERVER,
    token: process.env.HAMLET_TOKEN,
    view: process.env.HAMLET_VIEW,
    argv: process.argv.slice(2),
}));
writeFileSync(process.env.APP_PID, String(process.pid));
${
    hold
        ? `process.on("SIGTERM", () => {
    writeFileSync(process.env.APP_TERMINATED, "SIGTERM");
    process.exit(0);
});
setInterval(() => {}, 1000);`
        : "process.exit(3);"
}
`,
    );
    chmodSync(app, 0o755);
    return {
        app,
        env: {
            HAMLET_APP: app,
            HAMLET_VIEW: dir,
            APP_CAPTURE: capture,
            APP_TERMINATED: terminated,
            APP_PID: pid,
        },
        captured: () => JSON.parse(readFileSync(capture, "utf8")) as Capture,
        terminated: () => readFileSync(terminated, "utf8"),
    };
}

async function ready(run: Run, home: string) {
    const file = join(home, "server.json");
    await vi.waitFor(
        () => {
            expect(run.err()).not.toContain("Error:");
            expect(existsSync(file)).toBe(true);
        },
        { timeout: 10_000 },
    );
    const { url, pid } = JSON.parse(readFileSync(file, "utf8")) as {
        url: string;
        pid: number;
    };
    expect(pid).toBe(run.child.pid);
    const {
        connection: { token },
    } = JSON.parse(readFileSync(join(home, "credentials.json"), "utf8")) as {
        connection: { token: string };
    };
    return { url, token };
}

function fauxScript(content: unknown = { responses: [] }): string {
    const script = join(scratch(), "script.json");
    writeFileSync(script, JSON.stringify(content));
    return script;
}

describe("the window commands", () => {
    it("serves a kept temporary home after the window closes, until SIGINT (S1, S13)", async () => {
        const tmp = scratch();
        const home = join(tmp, `hamlet-dev-${process.getuid?.()}`);
        const fake = fakeApp();
        const env = {
            ...fake.env,
            HAMLET_HOME: undefined,
            TMPDIR: tmp,
            HOME: scratch(),
        };
        const args = ["dev", "--faux", fauxScript()];
        const dev = hamlet(args, scratch(), undefined, env);
        const address = await ready(dev, home);
        expect(statSync(home).mode & 0o777).toBe(0o700);
        await vi.waitFor(() =>
            expect(fake.captured()).toEqual({
                server: address.url,
                token: address.token,
                view: join(home, "window"),
                argv: [],
            }),
        );
        await vi.waitFor(() => expect(dev.err()).toContain("still serving"));
        expect(dev.err()).toContain(home);
        expect(dev.err()).toContain(address.url);
        expect(dev.err() + dev.out()).not.toContain(address.token);
        const connection = await connect(address.url);
        try {
            connection.send({ type: "hello", v: 0, token: address.token });
            expect(await connection.next()).toMatchObject({ type: "state" });
        } finally {
            await connection.close();
        }

        dev.child.kill("SIGINT");
        expect(await dev.ended).toBe(0);
        expect(existsSync(join(home, "server.json"))).toBe(false);
        expect(existsSync(join(home, "home.sqlite"))).toBe(true);
        const again = hamlet(args, scratch(), undefined, env);
        expect((await ready(again, home)).token).toBe(address.token);
        await vi.waitFor(() => expect(again.err()).toContain("still serving"));
        again.child.kill("SIGINT");
        expect(await again.ended).toBe(0);
    }, 15_000);

    it.each(["group-writable", "world-writable", "symlink", "file"] as const)(
        "refuses a %s dev home before reading planted credentials",
        async (kind) => {
            const home = join(scratch(), "dev-home");
            const target = scratch();
            const planted = JSON.stringify({
                installationId: "planted",
                connection: { token: "planted-by-another-user" },
                models: {},
            });
            if (kind === "symlink") symlinkSync(target, home);
            else if (kind === "file") writeFileSync(home, "not a home");
            else {
                mkdirSync(home);
                chmodSync(home, kind === "group-writable" ? 0o770 : 0o707);
            }
            const contents = kind === "symlink" ? target : home;
            if (kind !== "file")
                writeFileSync(join(contents, "credentials.json"), planted);
            const fake = fakeApp();
            const dev = hamlet(
                ["dev", "--home", home, "--faux", fauxScript()],
                scratch(),
                undefined,
                fake.env,
            );
            let end: number | string | undefined;
            void dev.ended.then((code) => {
                end = code;
            });
            await vi.waitFor(() => expect(end).toBe(1), { timeout: 3000 });
            expect(dev.err()).toContain("unsafe scratch home");
            expect(dev.err()).toContain(home);
            expect(dev.err()).toContain(
                kind === "symlink"
                    ? "symbolic link"
                    : kind === "file"
                      ? "not a directory"
                      : "writable by group or others",
            );
            expect(existsSync(join(contents, "home.sqlite"))).toBe(false);
            expect(() => fake.captured()).toThrow();
            if (kind !== "file")
                expect(
                    readFileSync(join(contents, "credentials.json"), "utf8"),
                ).toBe(planted);
        },
    );

    it.each(["SIGINT", "SIGTERM"] as const)(
        "sends SIGTERM to a live window before stopping at %s",
        async (signal) => {
            const home = scratch();
            const fake = fakeApp(true);
            const dev = hamlet(
                ["dev", "--faux", fauxScript()],
                home,
                undefined,
                fake.env,
            );
            await ready(dev, home);
            await vi.waitFor(() =>
                expect(fake.captured().view).toBe(join(home, "window")),
            );

            dev.child.kill(signal);

            await vi.waitFor(() => expect(fake.terminated()).toBe("SIGTERM"), {
                timeout: 3000,
            });
            expect(await dev.ended).toBe(0);
            expect(existsSync(join(home, "server.json"))).toBe(false);
        },
        15_000,
    );

    it("uses HAMLET_HOME unless --home overrides it", async () => {
        const home = scratch();
        const other = scratch();
        const fake = fakeApp();
        const script = fauxScript();
        for (const [args, expected] of [
            [["dev", "--faux", script], home],
            [["dev", "--home", other, "--faux", script, "--port", "0"], other],
        ] as const) {
            const dev = hamlet([...args], home, undefined, fake.env);
            await ready(dev, expected);
            await vi.waitFor(() =>
                expect(fake.captured().view).toBe(join(expected, "window")),
            );
            dev.child.kill("SIGTERM");
            expect(await dev.ended).toBe(0);
            expect(existsSync(join(expected, "server.json"))).toBe(false);
        }
    }, 15_000);

    it("refuses the chief's ordinary home, including an alias to it", async () => {
        const user = scratch();
        const ordinary = join(user, ".hamlet");
        mkdirSync(ordinary);
        const alias = join(scratch(), "alias");
        symlinkSync(ordinary, alias);
        const fake = fakeApp();
        for (const chosen of [ordinary, alias]) {
            const dev = hamlet(
                ["dev", "--home", chosen, "--faux", fauxScript()],
                scratch(),
                undefined,
                { ...fake.env, HOME: user },
            );
            let end: number | string | undefined;
            void dev.ended.then((code) => {
                end = code;
            });
            await vi.waitFor(() => expect(end).toBe(1), { timeout: 3000 });
            expect(dev.err()).toContain("~/.hamlet");
            expect(dev.err()).toContain("scratch");
            expect(existsSync(join(ordinary, "home.sqlite"))).toBe(false);
        }
    }, 10_000);

    it("finds HAMLET_APP, PATH, release and debug in order, and names how to build a missing app", async () => {
        const repo = scratch();
        const module = join(repo, "server", "src", "cli", "window.ts");
        mkdirSync(dirname(module), { recursive: true });
        copyFileSync(new URL("../src/cli/window.ts", import.meta.url), module);
        copyFileSync(
            new URL("../src/cli/process.ts", import.meta.url),
            join(dirname(module), "process.ts"),
        );
        writeFileSync(
            join(repo, "server", "package.json"),
            '{"type":"module"}',
        );
        const { findApp } = (await import(
            pathToFileURL(module).href
        )) as typeof import("../src/cli/window.ts");
        const fake = fakeApp();
        const release = join(repo, "target", "release", "hamlet-app");
        const debug = join(repo, "target", "debug", "hamlet-app");
        for (const file of [release, debug]) {
            mkdirSync(dirname(file), { recursive: true });
            copyFileSync(fake.app, file);
            chmodSync(file, 0o755);
        }
        expect(
            findApp({ HAMLET_APP: "/chosen/app", PATH: dirname(fake.app) }),
        ).toBe("/chosen/app");
        expect(findApp({ PATH: dirname(fake.app) })).toBe(fake.app);
        const empty = { PATH: scratch() };
        expect(findApp(empty)).toBe(release);
        chmodSync(release, 0o644);
        expect(findApp(empty)).toBe(debug);
        rmSync(debug);
        expect(() => findApp(empty)).toThrow(
            "No hamlet-app found. Build it with cargo build --bin hamlet-app, or set HAMLET_APP.",
        );
    });

    it("passes the local server and token only in the app's environment and exits with its code", async () => {
        const home = scratch();
        const served = await server(home, { responses: [] });
        const fake = fakeApp();
        const run = hamlet(["window"], home, undefined, fake.env);

        expect(await run.ended).toBe(3);
        const { token } = JSON.parse(
            readFileSync(join(home, "credentials.json"), "utf8"),
        ).connection;
        expect(fake.captured()).toEqual({
            server: served.url,
            token,
            view: fake.env.HAMLET_VIEW,
            argv: [],
        });
        expect(run.out() + run.err()).not.toContain(token);
    });

    it("names server and dev when no local server is running, and requires a remote token", async () => {
        const home = scratch();
        const fake = fakeApp();
        const missing = hamlet(["window"], home, undefined, {
            ...fake.env,
            HAMLET_SERVER: undefined,
        });
        expect(await missing.ended).toBe(1);
        expect(missing.err()).toContain("hamlet server");
        expect(missing.err()).toContain("hamlet dev");
        expect(missing.err().match(/hamlet server/g)).toHaveLength(1);
        expect(missing.err()).not.toContain("npm run");

        const remote = hamlet(["window"], home, undefined, {
            ...fake.env,
            HAMLET_SERVER: "ws://127.0.0.1:1",
            HAMLET_TOKEN: undefined,
        });
        expect(await remote.ended).toBe(1);
        expect(remote.err()).toBe(
            "Error: HAMLET_SERVER needs HAMLET_TOKEN, its home's token\n",
        );
    });

    it("reports a window launch failure without server advice", async () => {
        const app = join(scratch(), "missing-app");
        const run = hamlet(["window"], scratch(), undefined, {
            HAMLET_APP: app,
            HAMLET_VIEW: scratch(),
            HAMLET_SERVER: "ws://127.0.0.1:1",
            HAMLET_TOKEN: "fake-token",
        });
        expect(await run.ended).toBe(1);
        expect(run.err()).toContain(app);
        expect(run.err()).not.toContain("hamlet server");
        expect(run.err()).not.toContain("hamlet dev");
    });

    it("launches the window without reading the chief's stdin", async () => {
        const fake = fakeApp();
        const marker = join(scratch(), "stdin-read");
        const preload = join(scratch(), "observe-stdin.mjs");
        writeFileSync(
            preload,
            `import { writeFileSync } from "node:fs"; process.stdin.on("resume", () => writeFileSync(${JSON.stringify(marker)}, "read"));`,
        );
        const run = hamlet(["window"], scratch(), undefined, {
            ...fake.env,
            HAMLET_SERVER: "ws://127.0.0.1:7313",
            HAMLET_TOKEN: "remote-test-token",
            NODE_OPTIONS: `--import ${preload}`,
        });
        expect(await run.ended).toBe(3);
        expect(existsSync(marker)).toBe(false);
    });

    it("uses an environment-named server instead of the local home", async () => {
        const home = scratch();
        const fake = fakeApp();
        const run = hamlet(["window"], home, undefined, {
            ...fake.env,
            HAMLET_SERVER: "ws://127.0.0.1:7313",
            HAMLET_TOKEN: "remote-test-token",
        });
        expect(await run.ended).toBe(3);
        expect(fake.captured()).toEqual({
            server: "ws://127.0.0.1:7313",
            token: "remote-test-token",
            view: fake.env.HAMLET_VIEW,
            argv: [],
        });
        expect(run.err() + run.out()).not.toContain("remote-test-token");
    });

    it("closes the served home when its window cannot start", async () => {
        const home = scratch();
        const dev = hamlet(["dev", "--faux", fauxScript()], home, undefined, {
            HAMLET_APP: join(scratch(), "missing-app"),
            HAMLET_VIEW: scratch(),
        });
        expect(await dev.ended).toBe(1);
        expect(dev.err()).toContain("missing-app");
        expect(existsSync(join(home, "server.json"))).toBe(false);
        const next = await server(home, { responses: [] });
        next.child.kill("SIGTERM");
        expect(await next.ended).toBe(0);
    }, 15_000);

    it("points to the full usage for an unknown command", async () => {
        const run = hamlet(["unknown"], scratch());
        expect(await run.ended).toBe(1);
        expect(run.err()).toContain("unknown argument `unknown`");
        usage(run.err());
    });

    it.each([
        [["server", "--home", "/unused"], "unknown argument `--home`"],
        [["dev", "--home"], "--home needs the path of a scratch home"],
        [["dev", "--port", "65536"], "--port needs a port number"],
        [["dev", "--unknown"], "unknown argument `--unknown`"],
        [["window", "--port", "0"], "unknown argument `--port`"],
    ])("names invalid arguments %j", async (args, words) => {
        const run = hamlet(args as string[], scratch());
        expect(await run.ended).toBe(1);
        expect(run.err()).toContain(words as string);
    });

    it("exits at once at a second SIGINT while dev is closing the home", async () => {
        const home = scratch();
        const fake = fakeApp();
        const script = fauxScript({
            responses: [{ stallMs: 30_000, text: "Too late." }],
        });
        const dev = hamlet(
            ["dev", "--faux", script],
            home,
            undefined,
            fake.env,
        );
        const address = await ready(dev, home);
        await vi.waitFor(() => expect(dev.err()).toContain("still serving"));
        const { connection, messages } = await joined(
            address.url,
            address.token,
        );
        say(connection, "Take your time.");
        await vi.waitFor(() =>
            expect(
                messages.some(
                    (message) =>
                        message.type === "change" &&
                        message.changes.some(
                            (change) =>
                                change.type === "agent" && change.agent.busy,
                        ),
                ),
            ).toBe(true),
        );
        await new Promise((resolve) => setTimeout(resolve, 300));

        dev.child.kill("SIGINT");
        await vi.waitFor(() => expect(dev.err()).toContain("stopping;"));
        await new Promise((resolve) => setTimeout(resolve, 500));
        expect(dev.child.exitCode).toBeNull();
        const started = Date.now();
        dev.child.kill("SIGINT");
        expect(await dev.ended).toBe(130);
        expect(Date.now() - started).toBeLessThan(2000);
    }, 15_000);

    it("resolves the package executable through relative symlinks and paths with spaces", async () => {
        const dir = join(scratch(), "installed commands");
        mkdirSync(dir);
        const { bin } = JSON.parse(
            readFileSync(new URL("../package.json", import.meta.url), "utf8"),
        ) as { bin: { hamlet: string } };
        symlinkSync(
            fileURLToPath(new URL(`../${bin.hamlet}`, import.meta.url)),
            join(dir, "linked"),
        );
        symlinkSync("linked", join(dir, "hamlet"));
        const result = await promisify(execFile)(
            join(dir, "hamlet"),
            ["--help"],
            {
                cwd: scratch(),
                env: { ...process.env, HAMLET_HOME: scratch() },
            },
        );
        usage(result.stdout);
    });

    it("names every command through the executable and npm", async () => {
        const home = scratch();
        const help = hamlet(["--help"], home);
        expect(await help.ended).toBe(0);
        usage(help.out());

        const npm = await promisify(execFile)(
            "npm",
            ["run", "hamlet", "--", "--help"],
            {
                cwd: new URL("..", import.meta.url),
                env: { ...process.env, HAMLET_HOME: home },
            },
        );
        usage(npm.stdout);
    });
});
