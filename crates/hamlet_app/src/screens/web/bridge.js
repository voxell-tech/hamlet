// The bridge the agents' web tools act on a page through. It runs in
// an agent's page before the page's own scripts, keeps its own hold
// of `window.cef`, and answers each ask the window emits as `hamlet`
// with an emit of the same `nonce`.
(() => {
  const cef = window.cef;
  if (!cef || window.__hamlet) {
    return;
  }
  Object.defineProperty(window, "__hamlet", { value: true });

  // What an agent can act on: links, buttons and fields.
  const ACTIVE =
    "a[href], button, input, textarea, select, summary, " +
    "[role=button], [role=link], [role=checkbox], [role=tab], " +
    "[contenteditable=''], [contenteditable=true], [onclick]";

  const shown = (element) => {
    if (!element.getClientRects().length) {
      return false;
    }
    const style = getComputedStyle(element);
    return style.visibility !== "hidden" && style.display !== "none";
  };

  const words = (text) => (text || "").replace(/\s+/g, " ").trim();

  const label = (element) =>
    words(
      element.getAttribute("aria-label") ||
        element.innerText ||
        element.value ||
        element.getAttribute("placeholder") ||
        element.getAttribute("title") ||
        element.getAttribute("alt") ||
        element.getAttribute("name") ||
        "",
    ).slice(0, 80);

  const kind = (element) => {
    const tag = element.tagName.toLowerCase();
    if (tag === "a") return "link";
    if (tag === "input") return `field ${element.type || "text"}`;
    if (tag === "textarea") return "field text";
    if (tag === "select") return "choice";
    if (element.isContentEditable) return "field text";
    return element.getAttribute("role") || "button";
  };

  const line = (element, number) => {
    let said = `[${number}] ${kind(element)} "${label(element)}"`;
    if (element.tagName === "A") {
      said += ` -> ${element.getAttribute("href")}`;
    }
    if ("value" in element && element.tagName !== "BUTTON") {
      said += ` (value "${words(String(element.value)).slice(0, 80)}")`;
    }
    if (element.disabled) {
      said += " (disabled)";
    }
    return said;
  };

  const acts = {
    read({ html }) {
      document
        .querySelectorAll("[data-hamlet]")
        .forEach((element) => element.removeAttribute("data-hamlet"));
      const elements = [...document.querySelectorAll(ACTIVE)].filter(shown);
      elements.forEach((element, at) =>
        element.setAttribute("data-hamlet", String(at + 1)),
      );
      const head = `Title: ${document.title}\nAddress: ${location.href}\n`;
      if (html) {
        return `${head}\n${document.documentElement.outerHTML}`;
      }
      const listed = elements.map((element, at) => line(element, at + 1));
      return (
        `${head}\n${(document.body && document.body.innerText) || ""}\n\n` +
        `Elements:\n${listed.join("\n") || "(none)"}`
      );
    },
  };

  cef.listen("hamlet", async (asked) => {
    const ask = typeof asked === "string" ? JSON.parse(asked) : asked;
    try {
      const text = await acts[ask.act](ask);
      cef.emit({ hamlet: ask.nonce, failed: false, text: String(text) });
    } catch (error) {
      cef.emit({ hamlet: ask.nonce, failed: true, text: String(error) });
    }
  });
})();
