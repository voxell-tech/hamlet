import { defineDocFamily } from "@earendil-works/pi-durable";
import type { Amount } from "./money.ts";
import { zero } from "./money.ts";

/** Each H12 task's spend, unaffected by periods, resets or rewinds. */
export const TaskDoc = defineDocFamily<
    { spent: Amount; messages: number },
    string
>({
    kind: "hamlet.task",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ spent: zero(), messages: 0 }),
});
