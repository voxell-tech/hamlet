import { readFile, rename, rm, writeFile } from "node:fs/promises";
import type { CreateModelsOptions } from "@earendil-works/pi-ai/models";

import { atEndpoint } from "./endpoints.ts";

type ModelsStore = NonNullable<CreateModelsOptions["modelsStore"]>;
type ModelsStoreEntry = NonNullable<Awaited<ReturnType<ModelsStore["read"]>>>;

/** The last-known provider lists, rewritten whole before pi-ai publishes a refresh. */
export class FileModelsStore implements ModelsStore {
    readonly #file: string;
    readonly #endpoints = new Map<string, string>();
    #entries: Record<string, ModelsStoreEntry> | undefined;
    #line: Promise<unknown> = Promise.resolve();
    constructor(file: string) {
        this.#file = file;
    }
    async #load(): Promise<Record<string, ModelsStoreEntry>> {
        if (this.#entries !== undefined) return this.#entries;
        try {
            this.#entries = JSON.parse(
                await readFile(this.#file, "utf8"),
            ) as Record<string, ModelsStoreEntry>;
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
            this.#entries = {};
        }
        return this.#entries;
    }
    /** The saved address of one endpoint, independent of process overrides. */
    endpoint(provider: string, baseUrl: string): void {
        this.#endpoints.set(provider, baseUrl);
    }
    #atEndpoint(provider: string, entry: ModelsStoreEntry): ModelsStoreEntry {
        const address = this.#endpoints.get(provider);
        return address === undefined
            ? entry
            : {
                  ...entry,
                  models: entry.models.map((model) =>
                      atEndpoint(model, address),
                  ),
              };
    }
    async read(provider: string): Promise<ModelsStoreEntry | undefined> {
        await this.#line;
        const entry = (await this.#load())[provider];
        return entry === undefined
            ? undefined
            : this.#atEndpoint(provider, entry);
    }
    write(provider: string, entry: ModelsStoreEntry): Promise<void> {
        return this.#change(async (entries) => {
            entries[provider] = structuredClone(
                this.#atEndpoint(provider, entry),
            );
        });
    }
    delete(provider: string): Promise<void> {
        return this.#change(async (entries) => {
            delete entries[provider];
        });
    }
    #change(
        change: (entries: Record<string, ModelsStoreEntry>) => Promise<void>,
    ): Promise<void> {
        const done = this.#line.then(async () => {
            const entries = { ...(await this.#load()) };
            await change(entries);
            const temporary = `${this.#file}.${process.pid}.tmp`;
            await rm(temporary, { force: true });
            await writeFile(
                temporary,
                `${JSON.stringify(entries, null, 2)}\n`,
                { flag: "wx", mode: 0o600 },
            );
            await rename(temporary, this.#file);
            this.#entries = entries;
        });
        this.#line = done.catch(() => {});
        return done;
    }
}
