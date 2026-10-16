import type { Change } from "./oversight/waits.ts";

/** The tool-task memo holding the gate's edit preview. */
export const CHANGE = "hamlet.change";

/** The most lines of a change that are kept. */
const KEPT = 200;

/** The most characters kept of a line or a path. */
const WIDE = 240;

/** The lines that stay shown before the first line that differs and after the last. */
const AROUND = 3;

/** The most pairs of lines compared one by one; a larger change is its old lines out and its new lines in. */
const PAIRS = 1_000_000;

type Mark = Change["lines"][number]["mark"];

/**
 * The change the call of tool `name` with `args` asks for, when it is an edit or a write, from its arguments alone. A
 * write changes `before`, the file's text when it is there, or else makes the file (C30).
 */
export function changeOf(
    name: string,
    args: unknown,
    before?: string,
): Change | undefined {
    if (typeof args !== "object" || args === null) return undefined;
    const { path } = args as { path?: unknown };
    if (typeof path !== "string") return undefined;
    if (name === "write") {
        const { content } = args as { content?: unknown };
        if (typeof content !== "string") return undefined;
        return changeBetween(path, before ?? "", content, before === undefined);
    }
    if (name !== "edit") return undefined;
    const { edits } = args as { edits?: unknown };
    if (!Array.isArray(edits)) return undefined;
    const sides = edits.map(
        (edit) => edit as { oldText?: unknown; newText?: unknown },
    );
    if (
        sides.some(
            (edit) =>
                typeof edit.oldText !== "string" ||
                typeof edit.newText !== "string",
        )
    ) {
        return undefined;
    }
    return changeBetween(
        path,
        sides.map((edit) => edit.oldText).join("\n"),
        sides.map((edit) => edit.newText).join("\n"),
        false,
    );
}

/** The change of `old` into `now` in the file at `path`, which `created` says the change makes (change.rs:133-202). */
export function changeBetween(
    path: string,
    old: string,
    now: string,
    created: boolean,
): Change {
    const lines = compare(old, now);
    const first = lines.findIndex(([mark]) => mark !== " ");
    const last = lines.findLastIndex(([mark]) => mark !== " ");
    const shown =
        first < 0
            ? []
            : lines.slice(
                  Math.max(first - AROUND, 0),
                  Math.min(last + 1 + AROUND, lines.length),
              );
    const added = lines.filter(([mark]) => mark === "+").length;
    const removed = lines.filter(([mark]) => mark === "-").length;
    return {
        path: clip(path),
        created,
        lines: shown
            .slice(0, KEPT)
            .map(([mark, text]) => ({ mark, text: clip(text) })),
        total: shown.length,
        added,
        removed,
        count: count(created, added, removed),
    };
}

/** The count of a change's lines, in words. */
function count(created: boolean, added: number, removed: number): string {
    const lines = (n: number) => (n === 1 ? "1 line" : `${n} lines`);
    if (created) return `New file, ${lines(added)}`;
    if (added === 0 && removed === 0) return "No line changed";
    if (removed === 0) return `${lines(added)} added`;
    if (added === 0) return `${lines(removed)} removed`;
    return `${lines(added)} added, ${removed} removed`;
}

/** The lines of `old` and `now` in a diff's order: a line of both once, a line of `old` alone before those of `now` in its place. */
function compare(old: string, now: string): [Mark, string][] {
    const a = linesOf(old);
    const b = linesOf(now);
    let head = 0;
    while (head < a.length && head < b.length && a[head] === b[head]) head++;
    let tail = 0;
    while (
        tail < a.length - head &&
        tail < b.length - head &&
        a[a.length - 1 - tail] === b[b.length - 1 - tail]
    ) {
        tail++;
    }
    const out = a.slice(head, a.length - tail);
    const into = b.slice(head, b.length - tail);
    const lines: [Mark, string][] = a.slice(0, head).map((line) => [" ", line]);
    if (out.length * into.length > PAIRS) {
        lines.push(...out.map((line): [Mark, string] => ["-", line]));
        lines.push(...into.map((line): [Mark, string] => ["+", line]));
    } else {
        // The longest run of lines both have, from each pair of places on.
        const wide = into.length + 1;
        const common = new Uint32Array((out.length + 1) * wide);
        for (let i = out.length - 1; i >= 0; i--) {
            for (let j = into.length - 1; j >= 0; j--) {
                common[i * wide + j] =
                    out[i] === into[j]
                        ? (common[(i + 1) * wide + j + 1] ?? 0) + 1
                        : Math.max(
                              common[(i + 1) * wide + j] ?? 0,
                              common[i * wide + j + 1] ?? 0,
                          );
            }
        }
        let i = 0;
        let j = 0;
        while (i < out.length && j < into.length) {
            if (out[i] === into[j]) {
                lines.push([" ", out[i] as string]);
                i++;
                j++;
            } else if (
                (common[(i + 1) * wide + j] ?? 0) >=
                (common[i * wide + j + 1] ?? 0)
            ) {
                lines.push(["-", out[i] as string]);
                i++;
            } else {
                lines.push(["+", into[j] as string]);
                j++;
            }
        }
        lines.push(...out.slice(i).map((line): [Mark, string] => ["-", line]));
        lines.push(...into.slice(j).map((line): [Mark, string] => ["+", line]));
    }
    lines.push(
        ...a.slice(a.length - tail).map((line): [Mark, string] => [" ", line]),
    );
    return lines;
}

/** The lines of `text` as Rust's `str::lines` gives them: a last newline ends a line, and a carriage return before one goes. */
export function linesOf(text: string): string[] {
    if (text === "") return [];
    const lines = text.split("\n");
    if (lines.at(-1) === "") lines.pop();
    return lines.map((line) =>
        line.endsWith("\r") ? line.slice(0, -1) : line,
    );
}

/** The first characters of `text`, at most {@link WIDE}. */
function clip(text: string): string {
    return [...text].slice(0, WIDE).join("");
}
