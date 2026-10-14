import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, it } from "vitest";
import { hamlet, server, tty } from "./cli-support.ts";
import { served } from "./connection-support.ts";
import { openaiFake } from "./openai-fake.ts";
import { scratch } from "./support.ts";

it("reads a key without echo, stores it only on the server, lists models and tries a hello", async () => {
    const endpoint = await openaiFake();
    const run = await served({
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
    });
    const env = {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    };
    const key = "sk-terminal-private-key";
    const login = hamlet(
        ["login", "openai", "--key"],
        run.home.dir,
        `${key}\n`,
        env,
    );
    expect(await login.ended).toBe(0);
    expect(login.out() + login.err()).not.toContain(key);
    expect(login.err()).toContain("Signed in to openai.");
    expect(
        JSON.parse(
            readFileSync(join(run.home.dir, "credentials.json"), "utf8"),
        ),
    ).toMatchObject({ models: { openai: { type: "api_key", key } } });
    const models = hamlet(["models"], run.home.dir, "", env);
    expect(await models.ended).toBe(0);
    expect(models.out()).toContain("local::org/large");
    const hello = hamlet(["models", "try"], run.home.dir, "", env);
    expect(await hello.ended).toBe(0);
    expect(hello.out()).toBe("Hello.\n");
    endpoint.listing({ data: [{ id: "small" }] });
    const fresh = hamlet(["models", "refresh"], run.home.dir, "", env);
    expect(await fresh.ended).toBe(0);
    expect(fresh.out()).toContain("local::small");
});

it("shows subscription URLs and codes and sends the prompted value back to its own login", async () => {
    const endpoint = await openaiFake();
    let received = "";
    const run = await served({
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
        logins: {
            chatgpt: {
                name: "ChatGPT",
                isSubscription: true,
                async login(interaction) {
                    interaction.notify({
                        type: "auth_url",
                        url: "https://example.invalid/login",
                        instructions: "Open this.",
                    });
                    interaction.notify({
                        type: "device_code",
                        verificationUri: "https://example.invalid/code",
                        userCode: "ABCD",
                    });
                    received = await interaction.prompt({
                        type: "secret",
                        message: "Paste a code.",
                    });
                    return {
                        type: "oauth",
                        access: "subscription-access",
                        refresh: "subscription-refresh",
                        expires: Date.now() + 60_000,
                    };
                },
                refresh: async (credential) => credential,
                toAuth: async (credential) => ({ apiKey: credential.access }),
            },
        },
    });
    const login = hamlet(["login", "chatgpt"], run.home.dir, "private-code\n", {
        HAMLET_SERVER: run.server.url,
        HAMLET_TOKEN: run.home.credentials.token,
    });
    expect(await login.ended).toBe(0);
    expect(received).toBe("private-code");
    expect(login.err()).toContain("https://example.invalid/login\nOpen this.");
    expect(login.err()).toContain("Code: ABCD");
    expect(login.err()).toContain("Signed in to chatgpt.");
    expect(login.out() + login.err()).not.toContain("private-code");
    expect(login.out() + login.err()).not.toContain("subscription-access");
});

it("keeps terminal key input private while allowing a typo to be corrected", async () => {
    const endpoint = await openaiFake();
    const run = await served({
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
    });
    const result = await tty(
        ["login", "openai", "--key"],
        run.home.dir,
        [{ after: "API key", write: "sk-tty-private-keZ\x7fy\n" }],
        {
            HAMLET_SERVER: run.server.url,
            HAMLET_TOKEN: run.home.credentials.token,
        },
    );
    expect(result.code).toBe(0);
    expect(result.output).toContain("Signed in to openai.");
    expect(result.output).not.toContain("sk-tty-private-ke");
    expect(await run.home.credentials.read("openai")).toMatchObject({
        type: "api_key",
        key: "sk-tty-private-key",
    });
}, 15_000);

it("prints the plain failure words when the chosen model answers no hello", async () => {
    const home = scratch();
    await server(home, { responses: [""] });
    const run = hamlet(["models", "try", "faux::faux-1"], home, "");
    expect(await run.ended).toBe(0);
    expect(run.out()).toBe(
        "That model did not answer a hello. Pick another one. (Error: the model answered with nothing)\n",
    );
    expect(run.err()).toBe("");
});
