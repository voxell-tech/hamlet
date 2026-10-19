// Offline providers behind the production runtime's provider-injection seam.
import { Console } from "node:console";
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
import { Channel, type Outgoing } from "../../src/channel.ts";
import { run } from "../../src/runtime.ts";

globalThis.console = new Console({
    stdout: process.stderr,
    stderr: process.stderr,
});
const dir = process.env.HAMLET_OAUTH_FIXTURE;
if (dir === undefined) throw new Error("No fixture directory");
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
const available = ["chatgpt", "openrouter", "openai"].map((id) =>
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
                          login: async (
                              interaction: Parameters<
                                  NonNullable<
                                      import("@earendil-works/pi-ai").ApiKeyAuth["login"]
                                  >
                              >[0],
                          ) => ({
                              type: "api_key" as const,
                              key: await interaction.prompt({
                                  type: "secret",
                                  message: "Paste your key.",
                              }),
                          }),
                          resolve: async ({
                              ctx,
                              credential,
                          }: Parameters<
                              import("@earendil-works/pi-ai").ApiKeyAuth["resolve"]
                          >[0]) => {
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
                              interaction: Parameters<
                                  import("@earendil-works/pi-ai").OAuthAuth["login"]
                              >[0],
                              options?: import("@earendil-works/pi-ai").LoginOptions,
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
                          refresh: async (
                              credential: OAuthCredential,
                          ): Promise<OAuthCredential> => {
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
                          toAuth: async (credential: OAuthCredential) => ({
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
const args = process.argv.slice(2);
const named = (flag: string) => args[args.indexOf(flag) + 1] ?? "";
class RecordedChannel extends Channel {
    override request(message: Outgoing): ReturnType<Channel["request"]> {
        if (
            message.type === "credential_request" &&
            message.action === "acquire"
        )
            appendFileSync(path("acquires"), `${message.scope}\n`);
        return super.request(message);
    }
}
await run(
    {
        runtime: named("--runtime"),
        store: named("--store"),
        fresh: args.includes("--fresh"),
        faux: undefined,
    },
    new RecordedChannel(process.stdin, process.stdout),
    available,
);
