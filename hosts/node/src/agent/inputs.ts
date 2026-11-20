/** The inputs a turn is given, as the model reads them. */
import type { Author, Input, Submit } from "./messages/agent.ts";
import { type Admission, NOTE_BREAK } from "./turn.ts";

/** An author's name, else its address. */
export function nameOf(author: Author, kind: "hire" | "worker"): string {
    return author.name ?? `${kind}#${author.agent}`;
}

/** `captions` say where a letter's or a question's files were saved. */
export function admissionOf(message: Submit, captions = ""): Admission {
    const { input } = message;
    const told = (text: string) =>
        captions === "" ? text : `${text}\n\n${captions}`;
    const base = {
        id: message.id,
        ...(message.task === undefined ? {} : { task: message.task }),
    };
    switch (input.type) {
        case "line": {
            const line = {
                ...base,
                ...attachedOf(input),
                kind: "line" as const,
            };
            return input.note === undefined
                ? { ...line, content: input.text }
                : {
                      ...line,
                      note: input.note,
                      content: `${input.text}${NOTE_BREAK}${input.note}`,
                  };
        }
        case "notice":
            return { ...base, kind: "notice", content: input.text };
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

/** A line's images and files as its item shows them. */
function attachedOf(
    line: Extract<Input, { type: "line" }>,
): Pick<Admission, "images" | "files"> {
    return {
        images: (line.images ?? []).map(({ id, at }) =>
            at === undefined ? { id } : { id, at },
        ),
        files: (line.files ?? []).map(({ id, at, name, size }) => ({
            id,
            ...(at === undefined ? {} : { at }),
            name: name ?? "",
            size: size ?? 0,
        })),
    };
}
