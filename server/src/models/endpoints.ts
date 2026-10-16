import type { BaseModel } from "@earendil-works/pi-ai";
import type { Price } from "./catalogue.ts";

/** Recognizes loopback and wildcard host addresses for local model pricing. */
export function isLocal(address: string): boolean {
    try {
        const host = new URL(
            address.includes("://") ? address : `http://${address}`,
        ).hostname;
        return (
            host === "localhost" ||
            host === "[::1]" ||
            host === "0.0.0.0" ||
            host.startsWith("127.")
        );
    } catch {
        return false;
    }
}

/** Accepts the lowercase letters, digits and hyphens allowed in endpoint ids. */
export function endpointName(name: string): boolean {
    return /^[a-z0-9-]+$/.test(name);
}

/** A custom model at an endpoint's current address and fallback price. */
export function atEndpoint<T extends BaseModel<string>>(
    model: T,
    baseUrl: string,
): T {
    const listed = (model as T & { listedPrice?: Price | null }).listedPrice;
    const rate = assumedRate(baseUrl);
    return {
        ...model,
        baseUrl,
        cost:
            listed == null
                ? {
                      ...model.cost,
                      input: rate,
                      output: rate,
                      cacheRead: rate,
                      cacheWrite: rate,
                  }
                : model.cost,
    };
}

/** Dollars per million tokens when the endpoint lists no price. */
export function assumedRate(url: string): number {
    return isLocal(url) ? 0.1 : 1;
}
