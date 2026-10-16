import { defineDocFamily } from "@earendil-works/pi-durable";

/** A chief's instruction and its objection hearings (H7, H10). */
export type Instruction = {
    task: string;
    chief: string;
    text: string;
    at: number;
    done: number | null;
    hearing: { agent: boolean; mayor: boolean; cleared: boolean };
};
/** The latest twenty instructions of each agent. */
export const InstructionsDoc = defineDocFamily<
    { given: Instruction[] },
    string
>({
    kind: "hamlet.instructions",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ given: [] }),
});
/** The instruction still protecting an agent from the mayor (H7). */
export function owed(
    doc: Readonly<{ given: readonly Instruction[] }> | undefined,
    now: number,
    holdMs: number,
): Instruction | undefined {
    return doc?.given.find(
        (i) => i.done === null || Math.max(0, now - i.done) < holdMs,
    );
}
