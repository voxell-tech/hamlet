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

  const pause = (ms) => new Promise((done) => setTimeout(done, ms));

  const element = (target) => {
    const found = /^\d+$/.test(String(target))
      ? document.querySelector(`[data-hamlet="${target}"]`)
      : document.querySelector(target);
    if (!found) {
      throw new Error(
        /^\d+$/.test(String(target))
          ? `No element is numbered ${target}; read the page again.`
          : `No element matches ${target}.`,
      );
    }
    found.scrollIntoView({ block: "center", inline: "center" });
    return found;
  };

  const named = (found) => `${kind(found)} "${label(found)}"`;

  // A field's value set as the browser would set it, so the page's
  // own code, React's included, sees the change.
  const fill = (found, text) => {
    if (found.isContentEditable) {
      found.textContent = text;
    } else {
      const prototype = Object.getPrototypeOf(found);
      const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
      if (setter) {
        setter.call(found, text);
      } else {
        found.value = text;
      }
    }
    found.dispatchEvent(new Event("input", { bubbles: true }));
    found.dispatchEvent(new Event("change", { bubbles: true }));
  };

  const enter = (found) => {
    for (const type of ["keydown", "keypress", "keyup"]) {
      found.dispatchEvent(
        new KeyboardEvent(type, {
          key: "Enter",
          code: "Enter",
          keyCode: 13,
          which: 13,
          bubbles: true,
          cancelable: true,
        }),
      );
    }
    const form = found.form || found.closest("form");
    if (form) {
      form.requestSubmit();
    }
  };

  const acts = {
    type({ target, text, submit }) {
      const found = element(target);
      const typable =
        found.isContentEditable ||
        found.tagName === "TEXTAREA" ||
        (found.tagName === "INPUT" &&
          !["button", "submit", "checkbox", "radio", "file"].includes(found.type));
      if (!typable) {
        throw new Error(`The ${named(found)} takes no text.`);
      }
      if (found.disabled || found.readOnly) {
        throw new Error(`The ${named(found)} cannot be changed.`);
      }
      found.focus({ preventScroll: true });
      fill(found, text);
      if (submit) {
        enter(found);
        return `Typed into the ${named(found)} and pressed Enter.`;
      }
      return `Typed into the ${named(found)}.`;
    },
    click({ target }) {
      const found = element(target);
      if (found.disabled) {
        throw new Error(`The ${named(found)} is disabled.`);
      }
      found.focus({ preventScroll: true });
      for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup"]) {
        found.dispatchEvent(
          new (type.startsWith("pointer") ? PointerEvent : MouseEvent)(type, {
            bubbles: true,
            cancelable: true,
            view: window,
          }),
        );
      }
      found.click();
      return `Clicked the ${named(found)}.`;
    },
    async wait({ text, selector, seconds }) {
      const until = Date.now() + seconds * 1000;
      const found = () =>
        selector
          ? document.querySelector(selector) !== null
          : ((document.body && document.body.innerText) || "").includes(text);
      while (!found()) {
        if (Date.now() > until) {
          const what = selector ? `an element ${selector} matches` : `"${text}"`;
          throw new Error(`The page did not show ${what} within ${seconds} s.`);
        }
        await pause(100);
      }
      return selector ? `The page has an element ${selector} matches.` : `The page shows "${text}".`;
    },
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
