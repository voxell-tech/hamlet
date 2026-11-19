/** The inputs a turn is given, as the model reads them. */
import type { Author, Submit } from "./messages/agent.ts";
import { type Admission, NOTE_BREAK } from "./turn.ts";

export function admissionOf(message: Submit): Admission {
    const { input } = message;
    const base = {
        id: message.id,
        ...(message.task === undefined ? {} : { task: message.task }),
    };
    const name = (author: Author, kind: "hire" | "worker") =>
        author.name ?? `${kind}#${author.agent}`;
    switch (input.type) {
        case "line":
            return input.note === undefined
                ? { ...base, kind: "line", content: input.text }
                : {
                      ...base,
                      kind: "line",
                      note: input.note,
                      content: `${input.text}${NOTE_BREAK}${input.note}`,
                  };
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
                content: `${name(input.from, "worker")} answers: ${input.text}`,
            };
        case "letter":
            return {
                ...base,
                kind: "letter",
                from: input.from,
                content: input.text,
            };
        case "question":
            return {
                ...base,
                kind: "question",
                from: input.from,
                question: input.letter,
                content: input.text,
            };
        case "answer":
            return {
                ...base,
                kind: "answer",
                from: input.from,
                content: `${name(input.from, "hire")} answers: ${input.text}`,
            };
    }
}
