import { defineDocFamily } from "@earendil-works/pi-durable";

/** Each agent's character, by `agentKey`, which no tool can reach (C13). */
export const CharacterDoc = defineDocFamily<{ text: string }, string>({
    kind: "hamlet.character",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ text: "" }),
});
