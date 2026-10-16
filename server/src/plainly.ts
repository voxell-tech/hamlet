/** A provider failure in Rust's plain words for a chief. */
export function plainly(error: string, hello = false): string {
    const lower = error.toLowerCase();
    if (/\bhttp 40[13]\b|^(error: )?40[13]\b|api key/.test(lower))
        return "That key was not accepted. Check that you copied all of it and that it is still active.";
    if (/\bhttp 402\b|^(error: )?402\b|credit|quota|billing/.test(lower))
        return "Your provider account has no credit left. Add some, then try again.";
    if (lower.includes("could not reach"))
        return "Could not reach it. Check your connection and the address.";
    if (lower.includes("in time"))
        return "It took too long to answer. Try again, or pick a smaller model.";
    const detail = [...error].slice(0, 160).join("");
    if (hello)
        return `That model did not answer a hello. Pick another one. (${detail})`;
    if (lower.includes("http 404"))
        return "That address did not answer as a model provider. It usually ends in /v1.";
    return `It answered with an error. Try again. (${detail})`;
}
