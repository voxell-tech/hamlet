/**
 * An edit's change as a chat shows it, a port of
 * `crates/hamlet_server/src/work/cut.rs`'s diff.
 */
import type { ToolChange } from "./messages/agent.ts";

/** The most lines of an edit's change kept, and characters of a line (`work/cut.rs`). */
const KEPT = 200;
const WIDE = 240;
const AROUND = 3;
/**
 * The most pairs of lines compared one by one; past it, the diff shows
 * every old line removed, then every new one added.
 */
const PAIRS = 1_000_000;

type Mark = ToolChange["lines"][number]["mark"];

/** An edit's change from its arguments alone, as `work/cut.rs`'s `change_of`. */
export function editOf(args: unknown): ToolChange | undefined {
    const { path, edits } = (args ?? {}) as { path?: unknown; edits?: unknown };
    if (typeof path !== "string" || !Array.isArray(edits)) return undefined;
    const old: string[] = [];
    const now: string[] = [];
    for (const edit of edits as { oldText?: unknown; newText?: unknown }[]) {
        if (
            typeof edit?.oldText !== "string" ||
            typeof edit.newText !== "string"
        ) {
            return undefined;
        }
        old.push(edit.oldText);
        now.push(edit.newText);
    }
    return changeBetween(path, old.join("\n"), now.join("\n"), false);
}

/** A write's change when it made its file, as `work/cut.rs`'s `change_of`. */
export function madeOf(args: unknown): ToolChange | undefined {
    const { path, content } = (args ?? {}) as {
        path?: unknown;
        content?: unknown;
    };
    if (typeof path !== "string" || typeof content !== "string") {
        return undefined;
    }
    return changeBetween(path, "", content, true);
}

/** The change of `old` into `now` at `path` (`work/cut.rs`). */
function changeBetween(
    path: string,
    old: string,
    now: string,
    created: boolean,
): ToolChange {
    const lines = compare(old, now);
    const first = lines.findIndex(([mark]) => mark !== "same");
    const last = lines.findLastIndex(([mark]) => mark !== "same");
    const shown =
        first === -1
            ? []
            : lines.slice(Math.max(0, first - AROUND), last + 1 + AROUND);
    const added = lines.filter(([mark]) => mark === "added").length;
    const removed = lines.filter(([mark]) => mark === "removed").length;
    const count = (n: number) => (n === 1 ? "1 line" : `${n} lines`);
    return {
        path: clip(path, WIDE),
        created,
        added,
        removed,
        total: shown.length,
        count: created
            ? `New file, ${count(added)}`
            : added === 0 && removed === 0
              ? "No line changed"
              : removed === 0
                ? `${count(added)} added`
                : added === 0
                  ? `${count(removed)} removed`
                  : `${count(added)} added, ${removed} removed`,
        lines: shown
            .slice(0, KEPT)
            .map(([mark, text]) => ({ mark, text: clip(text, WIDE) })),
    };
}

/** The lines of `old` and `now` in a diff's order (`work/cut.rs`). */
function compare(old: string, now: string): [Mark, string][] {
    const a = linesOf(old);
    const b = linesOf(now);
    let head = 0;
    while (head < a.length && head < b.length && a[head] === b[head]) {
        head += 1;
    }
    let tail = 0;
    while (
        tail < a.length - head &&
        tail < b.length - head &&
        a[a.length - 1 - tail] === b[b.length - 1 - tail]
    ) {
        tail += 1;
    }
    const out = a.slice(head, a.length - tail);
    const into = b.slice(head, b.length - tail);
    const lines: [Mark, string][] = a
        .slice(0, head)
        .map((line) => ["same", line]);
    if (out.length * into.length > PAIRS) {
        lines.push(...out.map((line): [Mark, string] => ["removed", line]));
        lines.push(...into.map((line): [Mark, string] => ["added", line]));
    } else {
        // The longest run of lines both have, from each pair of places on.
        const wide = into.length + 1;
        const common = new Uint32Array((out.length + 1) * wide);
        const at = (i: number, j: number) => common[i * wide + j] ?? 0;
        for (let i = out.length - 1; i >= 0; i -= 1) {
            for (let j = into.length - 1; j >= 0; j -= 1) {
                common[i * wide + j] =
                    out[i] === into[j]
                        ? at(i + 1, j + 1) + 1
                        : Math.max(at(i + 1, j), at(i, j + 1));
            }
        }
        let i = 0;
        let j = 0;
        while (i < out.length && j < into.length) {
            if (out[i] === into[j]) {
                lines.push(["same", out[i] ?? ""]);
                i += 1;
                j += 1;
            } else if (at(i + 1, j) >= at(i, j + 1)) {
                lines.push(["removed", out[i] ?? ""]);
                i += 1;
            } else {
                lines.push(["added", into[j] ?? ""]);
                j += 1;
            }
        }
        lines.push(
            ...out.slice(i).map((line): [Mark, string] => ["removed", line]),
        );
        lines.push(
            ...into.slice(j).map((line): [Mark, string] => ["added", line]),
        );
    }
    lines.push(
        ...a
            .slice(a.length - tail)
            .map((line): [Mark, string] => ["same", line]),
    );
    return lines;
}

function linesOf(text: string): string[] {
    if (text === "") return [];
    const lines = text.split("\n");
    if (lines.at(-1) === "") lines.pop();
    return lines.map((line) =>
        line.endsWith("\r") ? line.slice(0, -1) : line,
    );
}

export function clip(text: string, most: number): string {
    const chars = [...text];
    return chars.length > most ? chars.slice(0, most).join("") : text;
}
