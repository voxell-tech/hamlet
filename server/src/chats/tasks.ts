import { defineDocFamily } from "@earendil-works/pi-durable";
import type { Amount } from "../spend/money.ts";
import { zero } from "../spend/money.ts";

/** Each H12 task's spend, unaffected by periods, resets or rewinds. */
export const TaskDoc = defineDocFamily<
    { spent: Amount; messages: number; chief: string | null },
    string
>({
    kind: "hamlet.task",
    version: 2,
    migrate: (value) =>
        ({ ...value, chief: null }) as {
            spent: Amount;
            messages: number;
            chief: string | null;
        },
    scope: "session",
    family: true,
    initial: () => ({ spent: zero(), messages: 0, chief: null }),
});
