// Checked by `npm run check`'s tsc, never run: each `@ts-expect-error`
// fails the check when the line under it compiles.
import {
    type ActionsOf,
    Address,
    actor,
    type CallOptions,
} from "../src/actor/actor/mod.ts";
import { db } from "../src/actor/db/mod.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };

interface ReadFile {
    readonly path: string;
    readonly offset?: number;
}

interface WorkspaceActions {
    read_file(request: ReadFile): { readonly text: string };
}

const WORKSPACE = Address.of<WorkspaceActions>("workspace", "local");

const ticker = actor({
    actions: {
        arm: async (_c, _ms: number, _n: number) => {},
        pending: async () => 0,
    },
});

const TICKER = Address.of<ActionsOf<typeof ticker>>("ticker", "t1");

export const closed = actor({
    actions: {
        misspelt: async (c) => {
            // @ts-expect-error `cal` is not a member of the context.
            await c.cal(TICKER, "pending", [], CALL);
            // @ts-expect-error the action is `pending`.
            await c.call(TICKER, "pendng", [], CALL);
            // @ts-expect-error `arm` takes two numbers.
            await c.call(TICKER, "arm", ["300"], CALL);
            // @ts-expect-error an address is made by `Address.of`.
            await c.call({ kind: "ticker", key: "t1" }, "pending", [], CALL);
            await c.call(
                WORKSPACE,
                "read_file",
                // @ts-expect-error a field that is not set is left out.
                [{ path: "/a", offset: undefined }],
                CALL,
            );
            // @ts-expect-error the workspace has no `write_file`.
            await c.call(WORKSPACE, "write_file", [{ path: "/a" }], CALL);
            // @ts-expect-error the deadline is required.
            await c.call(WORKSPACE, "read_file", [{ path: "/a" }]);
            // @ts-expect-error `att` is not a member of the schedule.
            await c.schedule.att(Date.now(), "pending");
            const answer: { readonly text: string } = await c.call(
                WORKSPACE,
                "read_file",
                [{ path: "/a" }],
                CALL,
            );
            return answer;
        },
    },
});

export const closedRows = actor({
    db: db(),
    actions: {
        counted: async (c) => {
            // @ts-expect-error `run` answers a count, not rows.
            const rows: { id: number }[] = await c.db.run("DELETE FROM notes");
            // @ts-expect-error `getRow` is not a member of the client.
            await c.db.getRow("SELECT 1");
            return rows;
        },
    },
});
