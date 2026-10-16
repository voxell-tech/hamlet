import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { expect, it, vi } from "vitest";
import { closedPolicy } from "../src/sandbox/reach.ts";
import { encode } from "../src/sandbox/wire.ts";
import { platformSandbox, SandboxEnv } from "../src/sandbox.ts";
import { houseEnv } from "./sandbox-support.ts";
import { closeAfter, context, scratch } from "./support.ts";

it("keeps every character in file paths and text lines", async () => {
    const { env } = houseEnv();
    closeAfter(() => env.cleanup(context));
    const name = 'a\u2028b\u2029\t\r\n\\".txt';
    const text = "one\u2028two\u2029three\nlast";
    expect(await env.writeFile(name, text, context)).toEqual({
        ok: true,
        value: undefined,
    });
    expect(await env.readTextFile(name, context)).toEqual({
        ok: true,
        value: text,
    });
    expect(await env.readTextLines(name, undefined, context)).toEqual({
        ok: true,
        value: ["one\u2028two\u2029three", "last"],
    });
    expect(await env.listDir(".", context)).toMatchObject({
        ok: true,
        value: [{ name }],
    });
});

it("escapes Unicode separators in a wire frame", () => {
    expect(encode({ text: "a\u2028b\u2029c" })).toBe(
        '{"text":"a\\u2028b\\u2029c"}',
    );
});

/** A real sandbox behind a transport that damages exactly one request or reply. */
function damaged(direction: "request" | "reply" | "event"): SandboxEnv {
    const house = scratch();
    writeFileSync(join(house, "file"), "kept");
    const sandbox = platformSandbox();
    const env = new SandboxEnv({
        cwd: house,
        policy: closedPolicy(house),
        sandbox: {
            ...sandbox,
            command(...args) {
                const confined = sandbox.command(...args);
                return {
                    env: confined.env,
                    argv: [
                        process.execPath,
                        "--input-type=module",
                        "-e",
                        `
                        import { spawn } from 'node:child_process';
                        import { createInterface } from 'node:readline';
                        const [direction, spec] = process.argv.slice(1);
                        const { argv, env } = JSON.parse(spec);
                        const child = spawn(argv[0], argv.slice(1), { env });
                        let changed = false;
                        function damage(line, side) {
                            if (changed || (direction === 'event' ? side !== 'reply' || !Object.hasOwn(JSON.parse(line), 'event') : side !== direction)) return line;
                            changed = true;
                            return line.slice(0, line.indexOf(',')) + ',!';
                        }
                        createInterface({ input: process.stdin }).on('line', line => child.stdin.write(damage(line, 'request') + '\\n'));
                        createInterface({ input: child.stdout }).on('line', line => process.stdout.write(damage(line, 'reply') + '\\n'));
                        child.stderr.pipe(process.stderr);
                        child.stdin.on('error', () => {});
                        process.on('SIGTERM', () => child.kill());
                        child.on('close', () => process.exit());
                    `,
                        direction,
                        JSON.stringify(confined),
                    ],
                };
            },
        },
    });
    closeAfter(() => env.cleanup(context));
    return env;
}

it.each(["request", "reply"] as const)(
    "fails only the damaged %s, then keeps serving",
    async (direction) => {
        const env = damaged(direction);
        const first = env.readTextFile("file", context);
        const second = env.readTextFile("file", context);
        expect(await first).toMatchObject({
            ok: false,
            error: { code: "invalid" },
        });
        expect(await second).toEqual({ ok: true, value: "kept" });
        expect(await env.readTextFile("file", context)).toEqual({
            ok: true,
            value: "kept",
        });
    },
);

it.each(["process.exit(7)", "process.kill(process.pid, 'SIGKILL')"])(
    "replaces a helper after %s",
    async (ending) => {
        const house = scratch();
        writeFileSync(join(house, "file"), "kept");
        const sandbox = platformSandbox();
        let first = true;
        const env = new SandboxEnv({
            cwd: house,
            policy: closedPolicy(house),
            sandbox: {
                ...sandbox,
                command(...args) {
                    if (!first) return sandbox.command(...args);
                    first = false;
                    return { argv: [process.execPath, "-e", ending], env: {} };
                },
            },
        });
        closeAfter(() => env.cleanup(context));
        expect(await env.readTextFile("file", context)).toMatchObject({
            ok: false,
        });
        expect(await env.readTextFile("file", context)).toEqual({
            ok: true,
            value: "kept",
        });
    },
);

it("bounds a blocked FIFO request and serves the next call", async () => {
    const { house, env } = houseEnv({ wallSecs: 1 });
    closeAfter(() => env.cleanup(context));
    execFileSync("mkfifo", [join(house, "pipe")]);
    writeFileSync(join(house, "file"), "kept");
    expect(await env.writeFile("pipe", "blocked", context)).toMatchObject({
        ok: false,
    });
    expect(await env.readTextFile("file", context)).toEqual({
        ok: true,
        value: "kept",
    });
}, 10_000);

it("replaces a blocked helper even after its caller aborts", async () => {
    const { house, env } = houseEnv({ wallSecs: 1 });
    closeAfter(() => env.cleanup(context));
    execFileSync("mkfifo", [join(house, "pipe")]);
    writeFileSync(join(house, "file"), "kept");
    const abort = new AbortController();
    const blocked = env.writeFile("pipe", "blocked", {
        ...context,
        abortSignal: abort.signal,
    });
    await delay(200);
    abort.abort();
    expect(await blocked).toMatchObject({
        ok: false,
        error: { code: "aborted" },
    });
    await delay(1300);
    expect(await env.readTextFile("file", context)).toEqual({
        ok: true,
        value: "kept",
    });
}, 10_000);

it("keeps a helper working beyond a command's cumulative CPU limit", async () => {
    const { house, env } = houseEnv({ cpuSecs: 1 });
    closeAfter(() => env.cleanup(context));
    writeFileSync(join(house, "big"), Buffer.alloc(8 * 1024 * 1024, 97));
    for (let n = 0; n < 60; n++) {
        const read = await env.readBinaryFile("big", context);
        expect(read).toMatchObject({ ok: true });
        if (read.ok) expect(read.value.length).toBe(8 * 1024 * 1024);
    }
}, 30_000);

it("starts a helper without a command's process limit", async () => {
    const { env } = houseEnv({ maxProcs: 1 });
    closeAfter(() => env.cleanup(context));
    expect(await env.writeFile("file", "kept", context)).toMatchObject({
        ok: true,
    });
});

/** A sandbox whose process id can be observed from outside its policy. */
function observed(): { house: string; env: SandboxEnv; pid: () => number } {
    const house = scratch();
    const pidFile = join(scratch(), "pid");
    const sandbox = platformSandbox();
    writeFileSync(join(house, "file"), "kept");
    const env = new SandboxEnv({
        cwd: house,
        policy: closedPolicy(house),
        sandbox: {
            ...sandbox,
            command(...args) {
                const confined = sandbox.command(...args);
                return {
                    ...confined,
                    argv: [
                        "bash",
                        "-c",
                        'echo $$ > "$1"; shift; exec "$@"',
                        "bash",
                        pidFile,
                        ...confined.argv,
                    ],
                };
            },
        },
    });
    closeAfter(() => env.cleanup(context));
    return {
        house,
        env,
        pid: () => Number(readFileSync(pidFile, "utf8").trim()),
    };
}

function alive(pid: number): boolean {
    try {
        process.kill(pid, 0);
        return true;
    } catch {
        return false;
    }
}

it("reaps an idle helper after thirty seconds and starts one on demand", async () => {
    const { env, pid } = observed();
    expect(await env.readTextFile("file", context)).toEqual({
        ok: true,
        value: "kept",
    });
    const first = pid();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
        expect(await env.readTextFile("file", context)).toEqual({
            ok: true,
            value: "kept",
        });
        await vi.advanceTimersByTimeAsync(30_000);
    } finally {
        vi.useRealTimers();
    }
    await expect.poll(() => alive(first)).toBe(false);
    expect(await env.readTextFile("file", context)).toEqual({
        ok: true,
        value: "kept",
    });
    expect(pid()).not.toBe(first);
});

it("does not let an old reader address a new reader after idle replacement", async () => {
    const { env } = observed();
    const old = await env.openBinaryReader("file", undefined, context);
    if (!old.ok) throw old.error;
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
        await env.fileInfo("file", context);
        await vi.advanceTimersByTimeAsync(30_000);
    } finally {
        vi.useRealTimers();
    }
    const fresh = await env.openBinaryReader("file", undefined, context);
    if (!fresh.ok) throw fresh.error;
    expect(await old.value.read(0, 10, context)).toMatchObject({
        ok: false,
        error: { code: "invalid" },
    });
    expect(await fresh.value.read(0, 10, context)).toEqual({
        ok: true,
        value: new Uint8Array([107, 101, 112, 116]),
    });
    await old.value.close(context);
    expect(await fresh.value.info(context)).toMatchObject({
        ok: true,
        value: { size: 4 },
    });
    await fresh.value.close(context);
});

it.each(["SIGTERM", "SIGKILL"] as const)(
    "recovers after %s ends a running helper",
    async (signal) => {
        const { house, env, pid } = observed();
        expect(await env.readTextFile("file", context)).toMatchObject({
            ok: true,
        });
        execFileSync("mkfifo", [join(house, "pipe")]);
        const first = pid();
        const pending = env.writeFile("pipe", "blocked", context);
        await delay(200);
        process.kill(first, signal);
        expect(await pending).toMatchObject({ ok: false });
        expect(await env.readTextFile("file", context)).toEqual({
            ok: true,
            value: "kept",
        });
        expect(pid()).not.toBe(first);
    },
);

it("keeps an active watch alive, then reaps the helper after it closes", async () => {
    const { env, pid } = observed();
    const changes: unknown[] = [];
    const watch = await env.watch(
        [{ path: "file" }],
        (event) => changes.push(event),
        context,
    );
    if (!watch.ok) throw watch.error;
    const first = pid();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
        await vi.advanceTimersByTimeAsync(60_000);
    } finally {
        vi.useRealTimers();
    }
    expect(alive(first)).toBe(true);
    expect(await env.writeFile("file", "changed", context)).toMatchObject({
        ok: true,
    });
    await expect.poll(() => changes.length).toBeGreaterThan(0);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
        await watch.value.close(context);
        await vi.advanceTimersByTimeAsync(30_000);
    } finally {
        vi.useRealTimers();
    }
    await expect.poll(() => alive(first)).toBe(false);
});

it("reports a damaged watch event to only that watch and keeps serving", async () => {
    const env = damaged("event");
    const changes: unknown[] = [];
    const watch = await env.watch(
        [{ path: "file" }],
        (change) => changes.push(change),
        context,
    );
    if (!watch.ok) throw watch.error;
    await env.writeFile("file", "changed", context);
    await expect
        .poll(() => changes)
        .toMatchObject([{ error: { code: "invalid" } }]);
    expect(await env.readTextFile("file", context)).toEqual({
        ok: true,
        value: "changed",
    });
    await watch.value.close(context);
});
