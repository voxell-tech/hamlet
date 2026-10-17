import { mkdir, open, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { Credentials } from "../home/credentials.ts";
import { layoutOf } from "../home/layout.ts";
import { err, ok, type Result } from "../result.ts";
import { Scrubber } from "../scrub.ts";
import {
    connectChief,
    type DiscoveryFailure,
    reachHome,
    readServer,
    type ServerAddress,
    type ServerRecord,
} from "./client.ts";
import type { CommandIO } from "./io.ts";
import { startProcess } from "./process.ts";
import { findApp, launch } from "./window.ts";

type ServiceCommand = "start" | "stop" | "status";
type ServiceFailure =
    | DiscoveryFailure
    | { kind: "unresponsive"; home: string; pid: number }
    | {
          kind: "start";
          home: string;
          reason: "spawn" | "exit" | "timeout";
          tail: string;
      }
    | {
          kind: "home-files";
          home: string;
          operation: "prepare" | "remove-stale";
      }
    | { kind: "window" }
    | { kind: "remote-stop" }
    | { kind: "remote-token" }
    | { kind: "stop-signal"; home: string; pid: number }
    | { kind: "stopping"; home: string; pid: number };
type Found =
    | { kind: "absent" }
    | { kind: "stale"; server: ServerRecord }
    | { kind: "live"; server: ServerRecord; address: ServerAddress };

/** Runs the background service's commands without tying its lifetime to the terminal. */
export async function serviceCommand(
    command: ServiceCommand,
    dir: string,
    env: Readonly<NodeJS.ProcessEnv>,
    io: CommandIO,
): Promise<number> {
    const home = resolve(dir);
    const result = env.HAMLET_SERVER
        ? await remote(command, home, env, io)
        : await local(command, home, env, io);
    if (result.ok) return result.value;
    io.stderr.write(`Error: ${failureText(result.error)}\n`);
    return 1;
}

async function local(
    command: ServiceCommand,
    home: string,
    env: Readonly<NodeJS.ProcessEnv>,
    io: CommandIO,
): Promise<Result<number, ServiceFailure>> {
    const found =
        command === "start"
            ? await ensureServer(home, env)
            : await inspect(home, env);
    if (!found.ok) return found;
    switch (found.value.kind) {
        case "absent":
            io.stdout.write("hamlet: not running\n");
            return ok(command === "stop" ? 0 : 1);
        case "stale":
            if (command === "stop") {
                try {
                    await rm(join(home, "server.json"), { force: true });
                } catch {
                    return err({
                        kind: "home-files",
                        home,
                        operation: "remove-stale",
                    });
                }
            }
            io.stdout.write(
                `hamlet: stale server.json${command === "stop" ? " removed" : ""}; not running\n`,
            );
            return ok(command === "stop" ? 0 : 1);
        case "live": {
            const { server, address } = found.value;
            if (command === "stop") {
                const stopped = await stopServer(home, server);
                if (!stopped.ok) return stopped;
                io.stdout.write("hamlet: stopped\n");
                return ok(0);
            }
            io.stdout.write(
                `hamlet: running at ${server.url}; pid ${server.pid}; started ${server.started ?? "unknown"}; log ${join(home, "server.log")}\n`,
            );
            if (command === "start") {
                const opened = await openDetached(address, env);
                if (!opened.ok) return opened;
            }
            return ok(0);
        }
    }
}

function alive(pid: number): boolean {
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return !(
            error instanceof Error &&
            "code" in error &&
            error.code === "ESRCH"
        );
    }
}

async function inspect(
    home: string,
    env: Readonly<NodeJS.ProcessEnv>,
): Promise<Result<Found, ServiceFailure>> {
    const read = await readServer(home);
    if (!read.ok) return read;
    const server = read.value;
    if (server === null) return ok({ kind: "absent" });
    if (!alive(server.pid)) return ok({ kind: "stale", server });
    try {
        const address = await reachHome(home, env, server);
        if (await answers(address))
            return ok({ kind: "live", server, address });
    } catch {}
    if (!alive(server.pid)) return ok({ kind: "stale", server });
    return err({ kind: "unresponsive", home, pid: server.pid });
}

async function answers(address: ServerAddress): Promise<boolean> {
    try {
        const joined = await connectChief(address, AbortSignal.timeout(2000));
        await joined.connection.close();
        return true;
    } catch {
        return false;
    }
}

async function stopServer(
    home: string,
    server: ServerRecord,
): Promise<Result<void, ServiceFailure>> {
    try {
        process.kill(server.pid, "SIGTERM");
    } catch (error) {
        if (error instanceof Error && "code" in error && error.code === "ESRCH")
            return ok(undefined);
        return err({ kind: "stop-signal", home, pid: server.pid });
    }
    const deadline = Date.now() + 30_000;
    while (alive(server.pid)) {
        if (Date.now() >= deadline)
            return err({ kind: "stopping", home, pid: server.pid });
        await delay(100);
    }
    return ok(undefined);
}

async function remote(
    command: ServiceCommand,
    home: string,
    env: Readonly<NodeJS.ProcessEnv>,
    io: CommandIO,
): Promise<Result<number, ServiceFailure>> {
    if (command === "stop") return err({ kind: "remote-stop" });
    if (!env.HAMLET_TOKEN) return err({ kind: "remote-token" });
    const address = await reachHome(home, env);
    if (command === "status") {
        const live = await answers(address);
        io.stdout.write(
            `hamlet: ${live ? "running" : "not answering"} at ${address.url}\n`,
        );
        return ok(live ? 0 : 1);
    }
    const opened = await openDetached(address, env);
    return opened.ok ? ok(0) : opened;
}

async function ensureServer(
    home: string,
    env: Readonly<NodeJS.ProcessEnv>,
): Promise<Result<Found, ServiceFailure>> {
    const found = await inspect(home, env);
    if (!found.ok || found.value.kind === "live") return found;
    let log: Awaited<ReturnType<typeof open>>;
    try {
        await mkdir(home, { recursive: true, mode: 0o700 });
        log = await open(join(home, "server.log"), "a", 0o600);
    } catch {
        return err({ kind: "home-files", home, operation: "prepare" });
    }
    let child: ReturnType<typeof startProcess>;
    let logStart: number;
    try {
        logStart = (await log.stat()).size;
        child = startProcess(
            process.execPath,
            [
                "--enable-experimental-regexp-engine",
                "--disable-warning=ExperimentalWarning",
                fileURLToPath(new URL("../../bin/hamlet", import.meta.url)),
                "server",
                "--port",
                "0",
            ],
            env,
            undefined,
            { output: log.fd },
        );
        const spawned = await new Promise<boolean>((resolve) => {
            child.once("spawn", () => resolve(true));
            child.once("error", () => resolve(false));
        });
        if (!spawned)
            return err({
                kind: "start",
                home,
                reason: "spawn",
                tail: await logTail(home, env),
            });
    } finally {
        await log.close();
    }
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
        const ready = await inspect(home, env);
        if (ready.ok && ready.value.kind === "live") return ready;
        if (child.exitCode !== null || child.signalCode !== null) {
            const recent = await logTail(home, env, logStart);
            // A competing start may hold the home before publishing its address.
            if (
                !recent.includes("A server already runs for ") &&
                !recent.includes("EADDRINUSE")
            )
                return err({
                    kind: "start",
                    home,
                    reason: "exit",
                    tail: await logTail(home, env),
                });
        }
        await delay(100);
    }
    return err({
        kind: "start",
        home,
        reason: "timeout",
        tail: await logTail(home, env),
    });
}

async function logTail(
    home: string,
    env: Readonly<NodeJS.ProcessEnv>,
    from = 0,
): Promise<string> {
    try {
        const file = await open(join(home, "server.log"), "r");
        try {
            const { size } = await file.stat();
            const start = Math.max(from, size - 8192);
            const bytes = Buffer.alloc(Math.max(0, size - start));
            await file.read(bytes, 0, bytes.length, start);
            const credentials = await Credentials.readExisting(
                layoutOf(home).credentials,
                env.HAMLET_API_KEY,
            );
            const scrubber = new Scrubber([
                ...(credentials?.secrets() ?? []),
                env.HAMLET_API_KEY ?? "",
                env.HAMLET_TOKEN ?? "",
            ]);
            return (
                scrubber.push(bytes.toString("utf8")) + scrubber.flush()
            ).trimEnd();
        } finally {
            await file.close();
        }
    } catch {
        return "The end of server.log could not be read.";
    }
}

async function openDetached(
    address: ServerAddress,
    env: Readonly<NodeJS.ProcessEnv>,
): Promise<Result<void, ServiceFailure>> {
    try {
        const child = launch(findApp(env), { ...address, env, detached: true });
        return await new Promise((resolve) => {
            child.once("spawn", () => resolve(ok(undefined)));
            child.once("error", () => resolve(err({ kind: "window" })));
        });
    } catch {
        return err({ kind: "window" });
    }
}

function failureText(failure: ServiceFailure): string {
    switch (failure.kind) {
        case "remote-stop":
            return "Cannot stop HAMLET_SERVER here. A server elsewhere is stopped on its own machine.";
        case "remote-token":
            return "HAMLET_SERVER needs HAMLET_TOKEN, its home's token.";
        case "stop-signal":
            return `Cannot stop process ${failure.pid} for ${failure.home}: SIGTERM was refused. Inspect the process and its permissions on its machine.`;
        case "stopping":
            return `Process ${failure.pid} for ${failure.home} is still stopping after 30 seconds. Inspect ${join(failure.home, "server.log")}; wait, or force it with kill -KILL ${failure.pid} (unfinished work may need recovery). No SIGKILL was sent.`;
        case "start":
            return `Cannot start the server for ${failure.home}: ${failure.reason === "timeout" ? "it did not become ready within 15 seconds" : failure.reason === "exit" ? "the server exited before becoming ready" : "the process could not start"}. Inspect ${join(failure.home, "server.log")} and hamlet status before trying again.\n${failure.tail}`;
        case "home-files":
            return `Cannot ${failure.operation === "prepare" ? "prepare the home and server.log" : "remove stale server.json"} for ${failure.home}. Check the folder and its permissions before trying again.`;
        case "window":
            return "Cannot open a window. Build hamlet-app or set HAMLET_APP to its executable, then open again. No stop was requested; hamlet status reports the server.";
        case "server-record":
            return `Cannot read server.json for ${failure.home}. Inspect the file before starting or stopping its server.`;
        case "unresponsive":
            return `Process ${failure.pid} for ${failure.home} is alive but does not answer with the home's token; it is not ours to touch. Inspect it on its machine; no signal was sent.`;
        default: {
            const unreachable: never = failure;
            throw new Error(`Unknown service failure: ${unreachable}`);
        }
    }
}
