/** The inputs a turn is given, as the model reads them. */
import type { Submit } from "./messages/agent.ts";
import { type Admission, NOTE_BREAK } from "./turn.ts";

export function admissionOf(message: Submit): Admission {
    const { input } = message;
    const base = {
        id: message.id,
        ...(message.task === undefined ? {} : { task: message.task }),
    };
    const from = (author: { agent: number; name?: string }) =>
        author.name === undefined
            ? `\`hire#${author.agent}\``
            : `${author.name} (\`hire#${author.agent}\`)`;
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
        // As `crates/hamlet_server/src/talk/letters.rs:334` words it, with
        // the worker as `society/addresses.rs:182` names it.
        case "worker_answer":
            return {
                ...base,
                kind: "worker-answer",
                from: { agent: input.from.agent },
                content: `worker#${input.from.agent} answers: ${input.text}`,
            };
        case "letter":
            return {
                ...base,
                kind: "letter",
                from: input.from,
                content: `A letter from ${from(input.from)}:\n\n${input.text}`,
            };
        case "question":
            return {
                ...base,
                kind: "question",
                from: input.from,
                question: input.letter,
                content: `A question from ${from(input.from)}. Your answer to this turn is sent back as the reply:\n\n${input.text}`,
            };
        case "answer":
            return {
                ...base,
                kind: "answer",
                from: input.from,
                content: `${from(input.from)} answers your question:\n\n${input.text}`,
            };
    }
}
