/**
 * A Node host entry serving the agent with three offline providers in place
 * of today's, as `harnesses/pi/test/fixtures/oauth.ts` gives pi: `chatgpt`
 * signs in by subscription only, `openrouter` by key or subscription, and
 * `openai` by key only. A Rust host starts it through `HAMLET_NODE_HOST`.
 * Its files go in the folder `HAMLET_ACTOR_OAUTH_FIXTURE`: `started` once it
 * serves, `requests.jsonl` per model request, `login-ended` per sign-in, and
 * `refresh.jsonl` and `refresh-started` per rotation, which waits while
 * `hold-refresh` is there without `release-refresh`.
 */

import { appendFileSync, existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
    type AssistantMessage,
    createAssistantMessageEventStream,
    type Model,
    type OAuthCredential,
    type ProviderStreams,
} from "@earendil-works/pi-ai";
import { createProvider } from "@earendil-works/pi-ai/models";
import { hostedAgent } from "../../src/agent/host.ts";
import { fail, serve, take } from "../../src/entry.ts";

const ENTRY = "hamlet node host's OAuth fixture";

take(ENTRY);

const home = process.env.HAMLET_HOME;
if (home === undefined) fail(ENTRY, "HAMLET_HOME names no realm folder");
const dir = process.env.HAMLET_ACTOR_OAUTH_FIXTURE;
if (dir === undefined)
    fail(ENTRY, "HAMLET_ACTOR_OAUTH_FIXTURE names no folder");
const path = (name: string) => join(dir, name);

const model = (provider: string): Model<"openai-completions"> => ({
    provider,
    id: "offline",
    name: "Offline OAuth",
    api: "openai-completions",
    baseUrl: "http://127.0.0.1:1",
    reasoning: false,
    input: ["text"],
    cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 32768,
    maxTokens: 8192,
});

const stream: ProviderStreams["streamSimple"] = (
    selected,
    _context,
    options,
) => {
    const out = createAssistantMessageEventStream();
    void (async () => {
        appendFileSync(
            path("requests.jsonl"),
            `${JSON.stringify({ provider: selected.provider, key: options?.apiKey, headers: options?.headers })}\n`,
        );
        const message: AssistantMessage = {
            role: "assistant",
            api: selected.api,
            provider: selected.provider,
            model: selected.id,
            content: [{ type: "text", text: "" }],
            stopReason: "stop",
            timestamp: Date.now(),
            usage: {
                input: 7,
                output: 3,
                cacheRead: 0,
                cacheWrite: 0,
                totalTokens: 10,
                cost: {
                    input: 0.000007,
                    output: 0.000006,
                    cacheRead: 0,
                    cacheWrite: 0,
                    total: 0.000013,
                },
            },
        };
        const content = message.content[0];
        if (content?.type !== "text") throw new Error("text fixture");
        for (const delta of [
            "OAuth echoed test-oauth-access-",
            "before test-oauth-refresh-before test-oauth-access-after test-oauth-refresh-after test-other-provider-secret.",
        ]) {
            content.text += delta;
            out.push({
                type: "text_delta",
                contentIndex: 0,
                delta,
                partial: message,
            });
            await sleep(5);
        }
        out.push({ type: "done", reason: "stop", message });
        out.end(message);
    })();
    return out;
};

const providers = ["chatgpt", "openrouter", "openai"].map((id) =>
    createProvider({
        id,
        models: [model(id)],
        api: { stream, streamSimple: stream },
        fetchModels: async () => [model(id)],
        auth: {
            ...(id === "chatgpt"
                ? {}
                : {
                      apiKey: {
                          name: "Offline key",
                          login: async (interaction) => ({
                              type: "api_key" as const,
                              key: await interaction.prompt({
                                  type: "secret",
                                  message: "Paste your key.",
                              }),
                          }),
                          resolve: async ({ ctx, credential }) => {
                              if (
                                  (await ctx.env("OPENAI_API_KEY")) !==
                                  undefined
                              )
                                  throw new Error("Ambient auth escaped");
                              return credential?.key === undefined
                                  ? undefined
                                  : { auth: { apiKey: credential.key } };
                          },
                      },
                  }),
            ...(id === "openai"
                ? {}
                : {
                      oauth: {
                          name: "Offline subscription",
                          login: async (
                              interaction,
                              options,
                          ): Promise<OAuthCredential> => {
                              interaction.notify({
                                  type: "auth_url",
                                  url: "https://example.invalid/oauth",
                                  instructions: "Offline sign-in",
                              });
                              interaction.notify({
                                  type: "device_code",
                                  userCode: "SAFE-CODE",
                                  verificationUri:
                                      "https://example.invalid/device",
                              });
                              interaction.notify({
                                  type: "progress",
                                  message: "Waiting for the offline code",
                              });
                              try {
                                  const code = await interaction.prompt({
                                      type: "manual_code",
                                      message: "Offline code",
                                  });
                                  if (code.startsWith("reject"))
                                      throw new Error(`Rejected ${code}`);
                                  return {
                                      type: "oauth",
                                      access: "test-oauth-access-before",
                                      refresh: "test-oauth-refresh-before",
                                      expires:
                                          code === "expire"
                                              ? 0
                                              : Date.now() + 3_600_000,
                                      clientId: options?.getDeviceId?.(),
                                      scopes: ["offline", "profile"],
                                      agent: options?.agentName,
                                  };
                              } finally {
                                  appendFileSync(
                                      path("login-ended"),
                                      `${id}\n`,
                                  );
                              }
                          },
                          refresh: async (credential) => {
                              appendFileSync(
                                  path("refresh.jsonl"),
                                  `${JSON.stringify(credential)}\n`,
                              );
                              writeFileSync(path("refresh-started"), "started");
                              while (
                                  existsSync(path("hold-refresh")) &&
                                  !existsSync(path("release-refresh"))
                              )
                                  await sleep(10);
                              return {
                                  ...credential,
                                  access: "test-oauth-access-after",
                                  refresh: "test-oauth-refresh-after",
                                  expires: Date.now() + 3_600_000,
                              };
                          },
                          toAuth: async (credential) => ({
                              apiKey: credential.access,
                              headers: {
                                  "x-refresh-control": credential.refresh,
                              },
                          }),
                      },
                  }),
        },
    }),
);

const agent = hostedAgent(home, { signIn: false, claude: false, providers });
writeFileSync(path("started"), "started");
await serve(ENTRY, { agent });
