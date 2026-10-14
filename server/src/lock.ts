import { createHash } from "node:crypto";
import { existsSync, realpathSync } from "node:fs";
import { rm } from "node:fs/promises";
import { connect, createServer, type Socket } from "node:net";
import { isAbsolute, join, resolve } from "node:path";

/** This process's claim on a home (S2). */
export type Lock = {
    release(): Promise<void>;
};

/** Claims made in this process that are still being released, by socket. */
const releasing = new Map<string, Promise<void>>();

/** The Rust-spelled and canonical claims on `dir`, keeping every spelling of one home out (S2). */
export async function claim(dir: string): Promise<Lock> {
    const root = absolute(dir);
    const canonical = realpathSync.native(resolve(dir));
    const rust = await claimSocket(root);
    if (socketOf(root) === socketOf(canonical)) return rust;
    try {
        const normalized = await claimSocket(canonical);
        return {
            async release() {
                await normalized.release();
                await rust.release();
            },
        };
    } catch (error) {
        await rust.release();
        throw error;
    }
}

/** The claim on the socket named from `root`. */
async function claimSocket(root: string): Promise<Lock> {
    const name = socketOf(root);
    await releasing.get(name);
    if (await answers(name)) {
        throw new Error(`A server already runs for ${root}.`);
    }
    // Nobody answered, so a socket file left on disk is a dead holder's.
    if (!name.startsWith("\0")) await rm(name, { force: true });
    const connected = new Set<Socket>();
    const server = createServer((socket) => {
        // The Rust app asks the home's holder to show its window. A
        // server has none, so the request is read and dropped.
        connected.add(socket);
        socket.on("close", () => connected.delete(socket));
        socket.on("error", () => {});
        socket.resume();
    });
    await new Promise<void>((resolve, reject) => {
        server.once("error", reject);
        server.listen(name, () => {
            server.off("error", reject);
            resolve();
        });
    });
    let released: Promise<void> | undefined;
    return {
        release() {
            if (released === undefined) {
                released = new Promise<void>((resolve) => {
                    server.close(() => resolve());
                    for (const socket of connected) socket.destroy();
                }).finally(() => releasing.delete(name));
                releasing.set(name, released);
            }
            return released;
        },
    };
}

/**
 * The local socket a home is claimed with, named as the Rust app's interprocess names it: abstract on Linux, and
 * elsewhere in `/run/user/<uid>/` where that folder exists, else in `/tmp/`.
 */
function socketOf(root: string): string {
    const tag = createHash("sha256").update(root).digest("hex").slice(0, 12);
    const name = `hamlet-${tag}.sock`;
    if (process.platform === "linux") return `\0${name}`;
    const run = `/run/user/${process.getuid?.()}`;
    return existsSync(run) ? join(run, name) : join("/tmp", name);
}

/** `dir` made absolute as Rust's `std::path::absolute` makes it on Unix, which keeps `..` and a trailing slash. */
function absolute(dir: string): string {
    const parts = dir.split("/").filter((part) => part !== "" && part !== ".");
    const base = !isAbsolute(dir)
        ? process.cwd()
        : dir.startsWith("//") && !dir.startsWith("///")
          ? "//"
          : "/";
    const path = [base.replace(/\/$/, ""), ...parts].join("/") || "/";
    return dir.endsWith("/") && !path.endsWith("/") ? `${path}/` : path;
}

/** True when a process listens on the socket `name`. */
function answers(name: string): Promise<boolean> {
    return new Promise((resolve) => {
        const socket = connect(name);
        socket.once("connect", () => {
            socket.destroy();
            resolve(true);
        });
        socket.once("error", () => resolve(false));
    });
}
