import { Router } from "express";

const PAGES: Record<string, { title: string; text: string }> = {
  "/checkout/success": {
    title: "Thank you",
    text: "Your payment went through. Go back to Hamlet: it shows up in your library in a moment.",
  },
  "/checkout/cancel": {
    title: "Payment cancelled",
    text: "Nothing was charged. Go back to Hamlet to try again whenever you like.",
  },
  "/payouts/return": {
    title: "Payouts set up",
    text: "You can go back to Hamlet now. If Stripe still needs something, Hamlet will say so.",
  },
  "/payouts/refresh": {
    title: "Link expired",
    text: "This setup link is no longer valid. Go back to Hamlet and press the payouts button again.",
  },
};

/** The small pages Stripe sends a browser back to. */
export function pageRoutes(): Router {
  const router = Router();
  for (const [path, page] of Object.entries(PAGES)) {
    router.get(path, (_req, res) => {
      res.type("html").send(renderPage(page.title, page.text));
    });
  }
  return router;
}

function renderPage(title: string, text: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title} - Hamlet Bazaar</title>
<style>
body { font-family: system-ui, sans-serif; max-width: 32rem; margin: 4rem auto; padding: 0 1rem; color: #222; }
h1 { font-size: 1.5rem; }
</style>
</head>
<body>
<h1>${title}</h1>
<p>${text}</p>
</body>
</html>
`;
}
