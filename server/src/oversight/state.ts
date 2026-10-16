import type { Wait } from "../protocol.ts";
import type { RealmState } from "../realm.ts";
import type { View } from "../state.ts";
import { OversightDayDoc, OversightDoc, open, view } from "./waits.ts";

/** Wait state and the documents whose changes invalidate it. */
export const waitsView: View<
    Wait[],
    Pick<RealmState, "records" | "currentChats">
> = {
    docs: [OversightDoc.definition, OversightDayDoc.definition],
    async build(home, context, { records, currentChats }) {
        return (await open(home.harness, context)).map((wait) =>
            view(wait, records, currentChats),
        );
    },
};
