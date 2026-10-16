import { constants } from "node:fs";
import { lstat, open, readdir } from "node:fs/promises";
import { join } from "node:path";

/** A bounded regular skill paper, opened without following its final link. */
export async function paper(path: string): Promise<string> {
    try {
        const file = await open(
            path,
            constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
        );
        try {
            if (!(await file.stat()).isFile()) return "";
            const bytes = Buffer.alloc(64 * 1024);
            let read = 0;
            while (read < bytes.length) {
                const part = await file.read(
                    bytes,
                    read,
                    bytes.length - read,
                    read,
                );
                if (part.bytesRead === 0) break;
                read += part.bytesRead;
            }
            return bytes.subarray(0, read).toString("utf8");
        } finally {
            await file.close();
        }
    } catch {
        return "";
    }
}

/** One regular paper on a shelf, with its folder name. */
export type SkillPaper = { folder: string; text: string };
const papers = new Map<string, { key: string; text: string }>();

/** A no-link shelf and its bounded papers, read in the caller's sandbox. */
export async function shelf(
    dir: string,
    contents: boolean,
): Promise<SkillPaper[]> {
    if (!(await lstat(dir).catch(() => undefined))?.isDirectory()) return [];
    const found: SkillPaper[] = [];
    const kept = new Set<string>();
    for (const entry of await readdir(dir, { withFileTypes: true }).catch(
        () => [],
    )) {
        if (!entry.isDirectory()) continue;
        const path = join(dir, entry.name, "SKILL.md");
        const stat = await lstat(path).catch(() => undefined);
        if (!stat?.isFile()) continue;
        kept.add(path);
        let text = "";
        if (contents) {
            const key = [
                stat.dev,
                stat.ino,
                stat.mtimeMs,
                stat.ctimeMs,
                stat.size,
            ].join(":");
            const cached = papers.get(path);
            text = cached?.key === key ? cached.text : await paper(path);
            if (text !== "") papers.set(path, { key, text });
            else papers.delete(path);
        }
        found.push({ folder: entry.name, text });
    }
    for (const path of papers.keys())
        if (path.startsWith(`${dir}/`) && !kept.has(path)) papers.delete(path);
    return found.sort((a, b) =>
        a.folder < b.folder ? -1 : a.folder > b.folder ? 1 : 0,
    );
}
