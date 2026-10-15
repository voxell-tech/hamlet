import { mkdtemp } from "node:fs/promises";
import { join } from "node:path";
import { createInterface } from "node:readline";
import type { Context } from "@earendil-works/chord";
import {
    err,
    FileError,
    ok,
    type Result,
    type WatchChange,
} from "@earendil-works/pi-durable/env";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { shelf } from "../skill-paper.ts";
import { decode, encode, frameId } from "./wire.ts";

const context = {} as Context;
const tmp = process.argv[2] ?? "";
class FileEnv extends NodeExecutionEnv {
    override async createTempDir(
        prefix: string | undefined,
    ): Promise<Result<string, FileError>> {
        try {
            return ok(await mkdtemp(join(tmp, prefix ?? "tmp-")));
        } catch (error) {
            return err(new FileError("unknown", String(error)));
        }
    }
}
const env = new FileEnv({ cwd: process.cwd() });
const handles = new Map<number, object>();
const send = (value: unknown) => process.stdout.write(`${encode(value)}\n`);
const methods = new Set([
    "readTextLines",
    "readBinaryFile",
    "openTextLineReader",
    "openBinaryReader",
    "fileInfo",
    "listDir",
    "openDirReader",
    "canonicalPath",
    "writeFile",
    "appendFile",
    "truncateFile",
    "flushFile",
    "renameFile",
    "createDir",
    "remove",
    "createTempDir",
    "createTempFile",
]);
for await (const line of createInterface({ input: process.stdin })) {
    const id = frameId(line);
    if (id === undefined) continue;
    try {
        const { method, args, cwd } = decode(line) as {
            method: string;
            args: unknown[];
            cwd: string;
        };
        if (
            typeof method !== "string" ||
            !Array.isArray(args) ||
            typeof cwd !== "string"
        )
            throw new Error("Invalid file request");
        env.cwd = cwd;
        let result: Result<unknown, FileError>;
        if (method === "skills") {
            result = ok(await shelf(args[0] as string, args[1] as boolean));
        } else if (method === "handle") {
            const [handle, operation, ...params] = args as [
                number,
                string,
                ...unknown[],
            ];
            const target = handles.get(handle);
            if (target === undefined)
                result = err(new FileError("invalid", "Reader is closed"));
            else {
                result =
                    (await Reflect.apply(
                        Reflect.get(target, operation),
                        target,
                        [...params, context],
                    )) ?? ok(undefined);
                if (operation === "close") handles.delete(handle);
            }
        } else if (method === "watch") {
            const watched = await env.watch(
                args[0] as import("@earendil-works/pi-durable/env").WatchTarget[],
                (event: WatchChange) => send({ id, event }),
                context,
            );
            if (!watched.ok) result = watched;
            else {
                handles.set(id, watched.value);
                result = ok({
                    handle: id,
                    event: id,
                    mode: watched.value.mode,
                });
            }
        } else if (methods.has(method)) {
            result = await Reflect.apply(Reflect.get(env, method), env, [
                ...args,
                context,
            ]);
            if (result.ok && method.startsWith("open")) {
                handles.set(id, result.value as object);
                result = ok(id);
            }
        } else
            result = err(
                new FileError("not_supported", "Unknown file operation"),
            );
        send({ id, result });
    } catch (error) {
        send({ id, result: err(new FileError("invalid", String(error))) });
    }
}
