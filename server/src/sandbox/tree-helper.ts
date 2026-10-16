import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as path from "node:path";
import type { TreeListing } from "../house.ts";

const [mode, root = "", source = "", stage = "", entryCap, byteCap] =
    process.argv.slice(2);
const report: TreeListing & { limit?: "entries" | "bytes"; failed?: boolean } =
    { files: [], skipped: [] };
if (!mode || !root || (!source && mode !== "remove"))
    throw new Error("Missing tree operation");
const rel = (p: string) =>
    path.relative(root, p).split(path.sep).join("/") || ".";
let entries = 0,
    bytes = 0;
function parents(p: string) {
    const parts = path.relative(root, p).split(path.sep);
    let current = root;
    for (const part of parts.slice(0, -1)) {
        if (!part) continue;
        current = path.join(current, part);
        const info = fs.lstatSync(current);
        if (info.isSymbolicLink() || !info.isDirectory()) {
            report.skipped.push({
                path: rel(current),
                kind: info.isSymbolicLink() ? "link" : "special",
            });
            return false;
        }
    }
    return true;
}
function visit(p: string, dest?: string) {
    if (report.limit) return;
    if (mode === "stage" && ++entries > Number(entryCap)) {
        report.limit = "entries";
        return;
    }
    const info = fs.lstatSync(p);
    if (info.isDirectory()) {
        if (dest && dest !== stage) fs.mkdirSync(dest, { mode: 0o700 });
        for (const name of fs.readdirSync(p).sort())
            visit(path.join(p, name), dest && path.join(dest, name));
        if (dest) fs.chmodSync(dest, info.mode & 0o777);
    } else if (info.isFile()) {
        const file = fs.openSync(
            p,
            fs.constants.O_RDONLY |
                fs.constants.O_NOFOLLOW |
                fs.constants.O_NONBLOCK,
        );
        let output: number | undefined;
        try {
            const opened = fs.fstatSync(file);
            if (
                !opened.isFile() ||
                opened.dev !== info.dev ||
                opened.ino !== info.ino
            )
                throw new Error();
            if (mode === "stage" && bytes + opened.size > Number(byteCap)) {
                report.limit = "bytes";
                return;
            }
            if (dest) output = fs.openSync(dest, "wx", opened.mode & 0o777);
            const digest =
                    mode === "list" ? crypto.createHash("sha256") : undefined,
                chunk = Buffer.alloc(128 * 1024);
            let length = 0;
            for (;;) {
                const count = fs.readSync(file, chunk, 0, chunk.length, null);
                if (count === 0) break;
                bytes += count;
                length += count;
                if (mode === "stage" && bytes > Number(byteCap)) {
                    report.limit = "bytes";
                    return;
                }
                digest?.update(chunk.subarray(0, count));
                if (output !== undefined) {
                    let written = 0;
                    while (written < count)
                        written += fs.writeSync(
                            output,
                            chunk,
                            written,
                            count - written,
                        );
                }
            }
            if (output !== undefined)
                fs.fchmodSync(output, opened.mode & 0o777);
            if (mode === "list")
                report.files.push({
                    path: rel(p),
                    size: length,
                    sha256: digest?.digest("hex") ?? "",
                });
        } finally {
            if (output !== undefined) fs.closeSync(output);
            fs.closeSync(file);
        }
    } else
        report.skipped.push({
            path: rel(p),
            kind: info.isSymbolicLink() ? "link" : "special",
        });
}
function remove(p: string) {
    let info: fs.Stats;
    try {
        info = fs.lstatSync(p);
    } catch (error) {
        if (
            error instanceof Error &&
            "code" in error &&
            error.code === "ENOENT"
        )
            return;
        throw error;
    }
    if (info.isDirectory()) {
        for (const name of fs.readdirSync(p)) remove(path.join(p, name));
        fs.rmdirSync(p);
    } else fs.unlinkSync(p);
}
try {
    if (mode === "remove") {
        for (const p of process.argv.slice(4)) {
            if (parents(p)) remove(p);
        }
        if (report.skipped.length) report.failed = true;
    } else if (parents(source)) {
        visit(
            source,
            mode === "stage"
                ? source === root
                    ? stage
                    : path.join(stage, path.basename(source))
                : undefined,
        );
    }
} catch {
    report.failed = true;
}
process.stdout.write(JSON.stringify(report));
