/** The inputs a turn is given, as the model reads them. */
import type { ImageContent } from "@earendil-works/pi-ai";
import type { SubmissionDraft, UserInput } from "@earendil-works/pi-durable";
import type { Author, HeldCall, Input, Submit } from "./messages/agent.ts";
import { type Caption, NOTHING_RECEIVED, type Received } from "./parcels.ts";
import {
    type Admission,
    type CallEnd,
    KIND,
    keptOf,
    NOTE_BREAK,
} from "./turn.ts";

export function nameOf(author: Author, kind: "hire" | "worker"): string {
    return author.name ?? `${kind}#${author.agent}`;
}

export function admissionOf(
    message: Submit,
    received: Received = NOTHING_RECEIVED,
): Admission {
    const { input } = message;
    const told = (text: string) => placed(text, received.captions);
    const base = {
        id: message.id,
        ...(message.task === undefined ? {} : { task: message.task }),
    };
    switch (input.type) {
        case "line": {
            const said = told(input.text);
            const line = {
                ...base,
                ...attachedOf(input),
                ...(input.chief === undefined ? {} : { chief: input.chief }),
                ...(said === input.text ? {} : { text: input.text }),
                kind: "line" as const,
            };
            return input.note === undefined
                ? { ...line, content: given(said, received.images) }
                : {
                      ...line,
                      note: input.note,
                      content: given(
                          `${said}${NOTE_BREAK}${input.note}`,
                          received.images,
                      ),
                  };
        }
        case "notice":
            return {
                ...base,
                kind: "notice",
                content: input.text,
                ...(input.held === undefined
                    ? {}
                    : {
                          ended: {
                              call: input.held.call,
                              end: heldEnd(input.held.end, input.text),
                          },
                      }),
            };
        // A worker's answer and an answer are told as their author's, and a
        // letter and a question as their words, as
        // `crates/hamlet_server/src/talk/letters.rs` keeps them.
        case "worker_answer":
            return {
                ...base,
                kind: "worker-answer",
                from: input.from,
                content: `${nameOf(input.from, "worker")} answers: ${input.text}`,
            };
        case "letter":
            return {
                ...base,
                kind: "letter",
                from: input.from,
                content: told(input.text),
            };
        case "question":
            return {
                ...base,
                kind: "question",
                from: input.from,
                question: input.letter,
                content: told(input.text),
            };
        case "answer":
            return {
                ...base,
                kind: "answer",
                from: input.from,
                content: `${nameOf(input.from, "hire")} answers: ${input.text}`,
            };
    }
}

/**
 * The write that shows an input the realm withdrew, never read by the
 * model. Its request id is the input's, so it shows once.
 */
export function withdrawnInput(id: string, input: Input): SubmissionDraft {
    const admission = admissionOf({ id, input });
    return {
        type: "write",
        requestId: id,
        entry: {
            kind: `${KIND}${admission.kind}`,
            data: { ...keptOf(admission), text: input.text, at: Date.now() },
        },
    };
}

function heldEnd(end: HeldCall["end"], why: string): CallEnd {
    switch (end) {
        case "ran":
            return { type: "ran", exit: 0 };
        case "blocked":
            return { type: "blocked", why };
        case "aborted":
            return { type: "aborted" };
    }
}

/**
 * `text` with `captions` placed as `Attached::place` in
 * `crates/hamlet_server/src/attached.rs` places them: each at its byte
 * offset, set apart by a space, and those with none after it.
 */
function placed(text: string, captions: readonly Caption[]): string {
    const bytes = Buffer.from(text);
    const cuts = [
        ...new Set(
            captions.flatMap(({ at }) => (at === undefined ? [] : [at])),
        ),
    ].sort((a, b) => a - b);
    const pieces: string[] = [];
    let from = 0;
    for (const cut of cuts) {
        pieces.push(bytes.subarray(from, cut).toString());
        from = cut;
    }
    pieces.push(bytes.subarray(from).toString());
    let content = "";
    for (const [index, cut] of cuts.entries()) {
        content += pieces[index];
        if (content !== "" && !/\s$/u.test(content)) content += " ";
        content += captions
            .filter(({ at }) => at === cut)
            .map(({ words }) => words)
            .join(" ");
        if (/^\S/u.test(pieces[index + 1] ?? "")) content += " ";
    }
    content += pieces.at(-1);
    const last = captions
        .filter(({ at }) => at === undefined)
        .map(({ words }) => words)
        .join("\n");
    if (last === "") return content;
    return content === "" ? last : `${content}\n\n${last}`;
}

function given(words: string, images: readonly ImageContent[]): UserInput {
    return images.length === 0
        ? words
        : [{ type: "text", text: words }, ...images];
}

/** A line's images and files as its item shows them. */
function attachedOf(
    line: Extract<Input, { type: "line" }>,
): Pick<Admission, "images" | "files"> {
    return {
        images: (line.images ?? []).map(({ image: { id, at } }) =>
            at === undefined ? { id } : { id, at },
        ),
        files: (line.files ?? []).map(
            ({ attached: { id, at, name, size } }) => ({
                id,
                ...(at === undefined ? {} : { at }),
                name,
                size,
            }),
        ),
    };
}
