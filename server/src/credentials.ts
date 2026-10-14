import { randomBytes, randomUUID } from "node:crypto";
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import type {
    Credential,
    CredentialInfo,
    CredentialStore,
} from "@earendil-works/pi-ai";

/** The contents of the credentials file. */
type Stored = {
    /** This installation of Hamlet's own id, for the providers it signs in to (S8). */
    installationId: string;
    /** The token a window connects with (S4). */
    connection: { token: string };
    /** Model credentials by pi-ai provider id. */
    models: { [provider: string]: Credential };
    git: { [name: string]: string };
};

/** The home's credentials file, readable only by its owner and rewritten whole on each change (S7). */
export class Credentials implements CredentialStore {
    readonly #file: string;
    #stored: Stored;
    /** The key `HAMLET_API_KEY` gives, held in memory alone. */
    readonly #apiKey: string | undefined;
    /** The last write, which the next one waits for. */
    #line: Promise<unknown> = Promise.resolve();

    private constructor(
        file: string,
        stored: Stored,
        apiKey: string | undefined,
    ) {
        this.#file = file;
        this.#stored = stored;
        this.#apiKey = apiKey;
    }

    /** The credentials in `file`, which is made when missing, beside `apiKey`, which is never written. */
    static async open(file: string, apiKey?: string): Promise<Credentials> {
        let text: string | undefined;
        try {
            text = await readFile(file, "utf8");
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
        }
        if (text !== undefined) {
            return new Credentials(file, JSON.parse(text) as Stored, apiKey);
        }
        const credentials = new Credentials(
            file,
            {
                installationId: randomUUID(),
                connection: { token: randomBytes(32).toString("hex") },
                models: {},
                git: {},
            },
            apiKey,
        );
        await credentials.#write();
        return credentials;
    }

    get installationId(): string {
        return this.#stored.installationId;
    }

    get token(): string {
        return this.#stored.connection.token;
    }

    /** Every credential value, to keep out of what is kept or shown (C32). */
    secrets(): string[] {
        const models = Object.values(this.#stored.models).flatMap((each) =>
            each.type === "api_key"
                ? [each.key ?? ""]
                : [each.access, each.refresh],
        );
        return [
            this.token,
            this.#apiKey ?? "",
            ...models,
            ...Object.values(this.#stored.git),
        ].filter((secret) => secret !== "");
    }

    async read(provider: string): Promise<Credential | undefined> {
        return this.#stored.models[provider];
    }

    async list(): Promise<readonly CredentialInfo[]> {
        return Object.entries(this.#stored.models).map(([provider, each]) => ({
            providerId: provider,
            type: each.type,
        }));
    }

    modify(
        provider: string,
        change: (
            current: Credential | undefined,
        ) => Promise<Credential | undefined>,
    ): Promise<Credential | undefined> {
        return this.#queue(async () => {
            const next = await change(this.#stored.models[provider]);
            if (next !== undefined) {
                this.#stored.models[provider] = next;
                await this.#write();
            }
            return this.#stored.models[provider];
        });
    }

    delete(provider: string): Promise<void> {
        return this.#queue(async () => {
            if (this.#stored.models[provider] === undefined) return;
            delete this.#stored.models[provider];
            await this.#write();
        });
    }

    /** Runs `work` once every earlier write is done. */
    #queue<T>(work: () => Promise<T>): Promise<T> {
        const done = this.#line.then(work);
        this.#line = done.catch(() => {});
        return done;
    }

    /** Writes the whole file to a temporary one first, so a crash leaves the old file whole. */
    async #write(): Promise<void> {
        const temporary = `${this.#file}.${process.pid}.tmp`;
        await rm(temporary, { force: true });
        await writeFile(
            temporary,
            `${JSON.stringify(this.#stored, null, 2)}\n`,
            { mode: 0o600, flag: "wx" },
        );
        await rename(temporary, this.#file);
    }
}
