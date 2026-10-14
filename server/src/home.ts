import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { Models } from "@earendil-works/pi-ai";
import {
    createRegistry,
    Harness,
    type Storage,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { CodingTools } from "@earendil-works/pi-durable/tools";
import { budget } from "./budget.ts";
import { Charter } from "./charter.ts";
import { gate } from "./gate.ts";
import { Letters } from "./letters.ts";
import { Mayor } from "./mayor.ts";

export type HomeOptions = {
    readonly models: Models;
    /** Opens the home's storage file; SQLite by default. */
    readonly openStorage?: (file: string) => Promise<Storage>;
};

/** Opens the home in `dir` and carries on the work it left unfinished (S3). */
export async function openHome(
    dir: string,
    options: HomeOptions,
    context: Context,
): Promise<Harness> {
    const registry = createRegistry();
    registry.install(CodingTools);
    registry.install(Charter);
    registry.install(Mayor);
    registry.install(Letters);
    const openStorage = options.openStorage ?? openNodeSqliteStorage;
    const storage = await openStorage(join(dir, "home.sqlite"));
    const harness = await Harness.open(
        storage,
        { models: options.models, registry },
        context,
    );
    registry.install(budget(harness));
    registry.install(gate(harness));
    harness.resume();
    return harness;
}
