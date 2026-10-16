import { plainly } from "./plainly.ts";

/** A settings edit refused before providers or saved settings change. */
export type SettingsFailure = {
    operation: "edit-settings";
    retrySafe: true;
} & (
    | { kind: "reflect-every-not-whole" }
    | {
          kind:
              | "endpoint-name-invalid"
              | "endpoint-builtin"
              | "endpoint-duplicate";
          name: string;
      }
    | { kind: "address-scheme"; reason: "invalid-url" | "scheme" }
    | { kind: "address-has-credentials" }
);
/** Chief-facing settings constraints, without endpoint credentials. */
export function settingsFailureText(error: SettingsFailure): string {
    switch (error.kind) {
        case "reflect-every-not-whole":
            return "reflectEvery must be a whole number";
        case "endpoint-name-invalid":
        case "endpoint-builtin":
        case "endpoint-duplicate":
            return `\`${error.name}\` is not a unique custom endpoint name.`;
        case "address-scheme":
            return error.reason === "invalid-url"
                ? "Invalid URL"
                : "The address should start with https:// or http://.";
        case "address-has-credentials":
            return "Keep credentials out of an endpoint's address.";
    }
    const unhandled: never = error;
    throw new Error(`Unclassified settings failure: ${unhandled}`);
}

/** A model card edit refused without replacing a chief's stored override. */
export type CatalogueFailure = {
    operation: "edit-catalogue";
    retrySafe: true;
} & ({ kind: "model-unknown"; name: string } | { kind: "price-negative" });
/** The chief's unchanged catalogue refusal words. */
export function catalogueFailureText(error: CatalogueFailure): string {
    switch (error.kind) {
        case "model-unknown":
            return `no model named \`${error.name}\``;
        case "price-negative":
            return "a cost is a number of zero or more";
    }
    const unhandled: never = error;
    throw new Error(`Unclassified catalogue failure: ${unhandled}`);
}

/** Provider text scrubbed at the external call before storage or display. */
export type ProviderFailure = {
    kind: "provider-refused";
    operation: "sign-in" | "check-model" | "check-connection";
    provider: string;
    reason: string;
    retrySafe: false;
};
/** Connection checks keep their plain words; sign-in progress keeps the provider's safe refusal. */
export function providerFailureText(error: ProviderFailure): string {
    switch (error.kind) {
        case "provider-refused":
            return error.operation === "sign-in"
                ? error.reason
                : plainly(error.reason, error.operation === "check-model");
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified provider failure: ${unhandled}`);
}

/** A refused sign-in or a reply that belongs to no active prompt. */
export type LoginFailure =
    | ProviderFailure
    | ({ operation: "sign-in"; retrySafe: false } & (
          | { kind: "method-not-offered" | "port-in-use"; provider: string }
          | { kind: "login-unknown" | "login-not-waiting"; login: string }
      ));
/** Safe sign-in words shared by progress updates and request replies. */
export function loginFailureText(error: LoginFailure): string {
    switch (error.kind) {
        case "provider-refused":
            return providerFailureText(error);
        case "method-not-offered":
            return `Signing in with ${error.provider} is not offered. Use an API key.`;
        case "port-in-use":
            return "Port 1455 is in use, probably by the Codex CLI or another sign-in. Cancel that sign-in and try again.";
        case "login-unknown":
            return "There is no sign-in here with that id.";
        case "login-not-waiting":
            return "That sign-in is not waiting for an answer.";
    }
    const unhandled: never = error;
    throw new Error(`Unclassified login failure: ${unhandled}`);
}

/** A model check needs a fully qualified, currently available model. */
export type ModelFailure =
    | ProviderFailure
    | ({ operation: "check-model"; retrySafe: true } & (
          | { kind: "model-name-invalid" | "model-not-found"; name: string }
          | { kind: "model-endpoint-missing"; name: string; provider: string }
      ));
/** Model lookup failures remain errors; provider check outcomes retain their notice text. */
export function modelFailureText(error: ModelFailure): string {
    switch (error.kind) {
        case "provider-refused":
            return providerFailureText(error);
        case "model-name-invalid":
            return `\`${error.name}\` names no endpoint. A model is named \`endpoint::model\``;
        case "model-endpoint-missing":
            return `\`${error.name}\` is served by the endpoint \`${error.provider}\`, which the settings do not have`;
        case "model-not-found":
            return `no model named \`${error.name}\``;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified model failure: ${unhandled}`);
}
