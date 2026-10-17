import { spawn } from "node:child_process";
import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { expect, it, vi } from "vitest";
import { WebSocketServer } from "ws";
import { hamlet, server } from "./cli-support.ts";
import { closeAfter, scratch } from "./support.ts";

function fixture() {
    const home = scratch();
    const capture = join(home, "windows.jsonl");
    const app = join(home, "hamlet-app");
    writeFileSync(
        app,
        `#!${process.execPath}
const { appendFileSync } = require("node:fs");
appendFileSync(process.env.APP_CAPTURE, JSON.stringify({
    server: process.env.HAMLET_SERVER, token: process.env.HAMLET_TOKEN,
    view: process.env.HAMLET_VIEW, argv: process.argv.slice(2), pid: process.pid,
}) + "\\n");
setInterval(() => {}, 1000);
`,
    );
    chmodSync(app, 0o755);
    const tracked = join(home, "server-pids");
    const tracker = join(home, "track.mjs");
    writeFileSync(
        tracker,
        `import { appendFileSync } from "node:fs";
if (process.argv.includes("server")) appendFileSync(process.env.SERVER_PIDS, process.pid + "\\n");
`,
    );
    const env = {
        HAMLET_APP: app,
        HAMLET_VIEW: join(home, "window"),
        APP_CAPTURE: capture,
        HAMLET_SERVER: "",
        HAMLET_TOKEN: "",
        SERVER_PIDS: tracked,
        NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ""} --import ${pathToFileURL(tracker).href}`,
    };
    const windows = (): {
        server: string;
        token: string;
        view: string;
        argv: string[];
        pid: number;
    }[] =>
        existsSync(capture)
            ? readFileSync(capture, "utf8")
                  .trim()
                  .split("\n")
                  .map((line) => JSON.parse(line))
            : [];
    closeAfter(async () => {
        for (const window of windows()) {
            try {
                process.kill(window.pid, "SIGTERM");
            } catch {}
        }
        const pids = existsSync(tracked)
            ? readFileSync(tracked, "utf8").trim().split("\n").map(Number)
            : [];
        for (const pid of pids) {
            try {
                process.kill(pid, "SIGTERM");
            } catch {
                continue;
            }
            let ended = false;
            for (let attempt = 0; attempt < 300; attempt++) {
                try {
                    process.kill(pid, 0);
                } catch {
                    ended = true;
                    break;
                }
                await delay(100);
            }
            if (!ended) {
                process.kill(pid, "SIGKILL");
                throw new Error(`Detached test server ${pid} did not stop`);
            }
        }
    });
    return { home, env, windows };
}

async function launch(f = fixture()) {
    const run = hamlet([], f.home, undefined, f.env);
    const code = await run.ended;
    expect(code, run.err()).toBe(0);
    await vi.waitFor(() => expect(f.windows()).toHaveLength(1));
    return { ...f, run };
}

it("starts a detached server and window, then reuses the same live server", async () => {
    const initial = fixture();
    writeFileSync(join(initial.home, "server.log"), "older log marker\n");
    const f = await launch(initial);
    const first = JSON.parse(readFileSync(join(f.home, "server.json"), "utf8"));
    const credentials = JSON.parse(
        readFileSync(join(f.home, "credentials.json"), "utf8"),
    );
    expect(f.windows()[0]).toMatchObject({
        server: first.url,
        token: credentials.connection.token,
        argv: [],
        view: f.env.HAMLET_VIEW,
    });
    expect(readFileSync(join(f.home, "server.log"), "utf8")).toMatch(
        /^older log marker\n/,
    );
    for (const window of f.windows()) process.kill(window.pid, "SIGTERM");
    const again = hamlet([], f.home, undefined, f.env);
    expect(await again.ended, again.err()).toBe(0);
    await vi.waitFor(() => expect(f.windows()).toHaveLength(2));
    expect(
        JSON.parse(readFileSync(join(f.home, "server.json"), "utf8")),
    ).toEqual(first);
    expect(again.out()).toContain(first.url);
    expect(again.out()).toContain(first.started);
}, 20_000);

it("replaces a dead server record before opening the window", async () => {
    const f = fixture();
    const dead = hamlet(["--help"], f.home);
    await dead.ended;
    writeFileSync(
        join(f.home, "server.json"),
        JSON.stringify({ url: "ws://127.0.0.1:1", pid: dead.child.pid }),
    );
    await launch(f);
    const record = JSON.parse(
        readFileSync(join(f.home, "server.json"), "utf8"),
    );
    expect(record.pid).not.toBe(dead.child.pid);
    expect(record.url).not.toBe("ws://127.0.0.1:1");
}, 20_000);

it("two concurrent starts share one detached server and both open windows", async () => {
    const f = fixture();
    const first = hamlet([], f.home, undefined, f.env);
    const second = hamlet([], f.home, undefined, f.env);
    expect(await first.ended, first.err()).toBe(0);
    expect(await second.ended, second.err()).toBe(0);
    await vi.waitFor(() => expect(f.windows()).toHaveLength(2));
    const record = JSON.parse(
        readFileSync(join(f.home, "server.json"), "utf8"),
    );
    for (const window of f.windows()) expect(window.server).toBe(record.url);
    expect(first.out()).toContain(record.url);
    expect(second.out()).toContain(record.url);
    expect(
        readFileSync(join(f.home, "server.log"), "utf8").match(
            /hamlet: serving /g,
        ),
    ).toHaveLength(1);
}, 20_000);

it("includes the end of the log when the detached server exits during startup", async () => {
    const f = fixture();
    const run = hamlet([], f.home, undefined, {
        ...f.env,
        HAMLET_REFLECT_EVERY: "not-a-number",
    });
    expect(await run.ended).toBe(1);
    expect(run.err()).toContain("exited before becoming ready");
    expect(run.err()).toContain("server.log");
    expect(run.err()).toContain("HAMLET_REFLECT_EVERY");
    expect(f.windows()).toHaveLength(0);
}, 20_000);

it("scrubs credentials from a failed start's log tail", async () => {
    const f = fixture();
    const key = "private-startup-key";
    const run = hamlet([], f.home, undefined, {
        ...f.env,
        HAMLET_API_KEY: key,
        HAMLET_REFLECT_EVERY: key,
    });
    expect(await run.ended).toBe(1);
    expect(run.err()).toContain("[REDACTED]");
    expect(run.err()).not.toContain(key);
    expect(existsSync(join(f.home, "credentials.json"))).toBe(false);
    const stored = fixture();
    const credential = {
        installationId: "test",
        connection: { token: "test-token" },
        models: { test: { type: "api_key", key } },
    };
    writeFileSync(
        join(stored.home, "credentials.json"),
        JSON.stringify(credential),
    );
    const failed = hamlet([], stored.home, undefined, {
        ...stored.env,
        HAMLET_REFLECT_EVERY: key,
    });
    expect(await failed.ended).toBe(1);
    expect(failed.err()).not.toContain(key);
    expect(failed.err()).toContain("[REDACTED]");
    expect(
        JSON.parse(readFileSync(join(stored.home, "credentials.json"), "utf8")),
    ).toEqual(credential);
}, 20_000);

it("times out a stalled start with the end of its log", async () => {
    const f = fixture();
    const preload = join(f.home, "stall.mjs");
    const pidFile = join(f.home, "stalled-pid");
    writeFileSync(
        preload,
        `import { writeFileSync } from "node:fs";
if (process.argv.includes("server")) {
    writeFileSync(process.env.STALLED_PID, String(process.pid));
    console.error("stalled bootstrap marker");
    await new Promise(resolve => setTimeout(resolve, 60_000));
}
`,
    );
    closeAfter(async () => {
        if (!existsSync(pidFile)) return;
        const pid = Number(readFileSync(pidFile, "utf8"));
        process.kill(pid, "SIGTERM");
        await vi.waitFor(() => expect(() => process.kill(pid, 0)).toThrow());
    });
    const run = hamlet([], f.home, undefined, {
        ...f.env,
        NODE_OPTIONS: `--import ${pathToFileURL(preload).href}`,
        STALLED_PID: pidFile,
    });
    expect(await run.ended).toBe(1);
    expect(run.err()).toContain("within 15 seconds");
    expect(run.err()).toContain("stalled bootstrap marker");
    expect(f.windows()).toHaveLength(0);
}, 20_000);

it("stops the live process completely, removes its record, and leaves the window alone", async () => {
    const f = await launch();
    const { pid } = JSON.parse(
        readFileSync(join(f.home, "server.json"), "utf8"),
    );
    const stop = hamlet(["stop"], f.home, undefined, f.env);
    expect(await stop.ended, stop.err()).toBe(0);
    expect(() => process.kill(pid, 0)).toThrow();
    expect(existsSync(join(f.home, "server.json"))).toBe(false);
    for (const window of f.windows())
        expect(() => process.kill(window.pid, 0)).not.toThrow();
    expect(stop.out()).toContain("stopped");
    const status = hamlet(["status"], f.home, undefined, f.env);
    expect(await status.ended).toBe(1);
    expect(status.out()).toContain("not running");
}, 20_000);

it("stops an absent or stale server without starting one", async () => {
    const home = scratch();
    const absent = hamlet(["stop"], home);
    expect(await absent.ended).toBe(0);
    expect(absent.out()).toContain("not running");
    const dead = hamlet(["--help"], home);
    await dead.ended;
    writeFileSync(
        join(home, "server.json"),
        JSON.stringify({ url: "ws://127.0.0.1:1", pid: dead.child.pid }),
    );
    const stale = hamlet(["stop"], home);
    expect(await stale.ended).toBe(0);
    expect(stale.out()).toContain("stale");
    expect(existsSync(join(home, "server.json"))).toBe(false);
});

it("opens a remote window without a local server, reports remote authentication, and refuses remote stop", async () => {
    const remote = scratch();
    const served = await server(remote, { responses: [] });
    const {
        connection: { token },
    } = JSON.parse(readFileSync(join(remote, "credentials.json"), "utf8"));
    const f = fixture();
    const env = { ...f.env, HAMLET_SERVER: served.url, HAMLET_TOKEN: token };
    const start = hamlet([], f.home, undefined, env);
    expect(await start.ended, start.err()).toBe(0);
    await vi.waitFor(() => expect(f.windows()).toHaveLength(1));
    expect(f.windows()[0]).toMatchObject({
        server: served.url,
        token,
        argv: [],
    });
    expect(existsSync(join(f.home, "server.json"))).toBe(false);
    expect(existsSync(join(f.home, "server.log"))).toBe(false);
    const status = hamlet(["status"], f.home, undefined, env);
    expect(await status.ended).toBe(0);
    expect(status.out()).toContain(served.url);
    const wrong = hamlet(["status"], f.home, undefined, {
        ...env,
        HAMLET_TOKEN: "wrong",
    });
    expect(await wrong.ended).toBe(1);
    expect(wrong.out()).toContain("not answering");
    const stop = hamlet(["stop"], f.home, undefined, env);
    expect(await stop.ended).toBe(1);
    expect(stop.err()).toContain("own machine");
    expect(() => process.kill(served.pid, 0)).not.toThrow();
    const missing = hamlet([], f.home, undefined, { ...env, HAMLET_TOKEN: "" });
    expect(await missing.ended).toBe(1);
    expect(missing.err()).toContain("HAMLET_TOKEN");
}, 20_000);

it("refuses a live process that never answers, without sending it a signal", async () => {
    const home = scratch();
    const signaled = join(home, "signaled");
    const innocent = spawn(
        process.execPath,
        [
            "-e",
            `process.on("SIGTERM", () => require("node:fs").writeFileSync(process.env.SIGNAL_FILE, "signal")); setInterval(() => {}, 1000);`,
        ],
        {
            env: { ...process.env, HAMLET_HOME: home, SIGNAL_FILE: signaled },
            stdio: "ignore",
        },
    );
    const ended = new Promise((resolve) => innocent.once("exit", resolve));
    closeAfter(async () => {
        innocent.kill("SIGKILL");
        await ended;
    });
    const blackhole = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    await new Promise((resolve) => blackhole.once("listening", resolve));
    closeAfter(async () => {
        for (const socket of blackhole.clients) socket.terminate();
        await new Promise<void>((resolve) => blackhole.close(() => resolve()));
    });
    const address = blackhole.address();
    if (address === null || typeof address === "string")
        throw new Error("No listening port");
    writeFileSync(
        join(home, "server.json"),
        JSON.stringify({
            url: `ws://127.0.0.1:${address.port}`,
            pid: innocent.pid,
        }),
    );
    writeFileSync(
        join(home, "credentials.json"),
        JSON.stringify({ connection: { token: "test" } }),
    );
    for (const args of [[], ["stop"], ["status"]]) {
        const run = hamlet(args, home);
        expect(await run.ended).toBe(1);
        expect(run.err()).toContain("alive but does not answer");
        expect(run.err()).toContain("not ours to touch");
        expect(existsSync(join(home, "server.json"))).toBe(true);
        expect(existsSync(signaled)).toBe(false);
        expect(() => process.kill(innocent.pid ?? 0, 0)).not.toThrow();
    }
}, 15_000);

it("reports an unserved home without starting anything", async () => {
    const run = hamlet(["status"], scratch());
    expect(await run.ended).toBe(1);
    expect(run.out()).toContain("not running");
    expect(run.err()).toBe("");
});

it("reports the live server, including its start time and log, and reads older records", async () => {
    const home = scratch();
    const served = await server(home, { responses: [] });
    const file = join(home, "server.json");
    const record = JSON.parse(readFileSync(file, "utf8"));
    expect(new Date(record.started).toISOString()).toBe(record.started);
    const run = hamlet(["status"], home);
    expect(await run.ended).toBe(0);
    expect(run.out()).toContain(served.url);
    expect(run.out()).toContain(String(served.pid));
    expect(run.out()).toContain(record.started);
    expect(run.out()).toContain(join(home, "server.log"));
    delete record.started;
    writeFileSync(file, JSON.stringify(record));
    const old = hamlet(["status"], home);
    expect(await old.ended).toBe(0);
    expect(old.out()).toContain("started unknown");
});

it("reports a dead process as stale without removing the record", async () => {
    const home = scratch();
    const dead = hamlet(["--help"], home);
    await dead.ended;
    writeFileSync(
        join(home, "server.json"),
        JSON.stringify({ url: "ws://127.0.0.1:1", pid: dead.child.pid }),
    );
    const run = hamlet(["status"], home);
    expect(await run.ended).toBe(1);
    expect(run.out()).toContain("stale");
    expect(existsSync(join(home, "server.json"))).toBe(true);
});
