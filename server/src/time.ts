/** The local day of the time `at`, as `YYYY-MM-DD`. */
export function localDay(at: number): string {
    const date = new Date(at);
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
