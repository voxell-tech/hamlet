---
status: accepted, amended by [0014](0014-claude-subscriptions-run-in-anthropics-own-program-under-pi.md) for Claude
---

# Subscription logins only where the provider allows them

Besides API keys, Hamlet lets a chief sign in with a subscription, but only
"Sign in with ChatGPT", which OpenAI offers to open-source apps: the app
registers itself on first sign-in, under its own name, with no client
secret, and a Claude subscription through pi's provider `claude`, where the
chief signs in inside Anthropic's own program (ADR 0014). pi-ai has the
login flows for other subscriptions too (Claude
Pro/Max, GitHub Copilot), but Anthropic's consumer terms appear to forbid
routing requests through a subscription in another app, and Copilot's terms
have not been checked. Those providers are reached with API keys.

## Consequences

- Hamlet signs in as itself, through pi-ai's `LoginOptions.agentName` and
  an installation ID of its own, and labels the action "Continue with
  ChatGPT", as OpenAI's guide asks.
- OpenAI offers sign-in on commercial websites only to selected partners, so
  a hosted Hamlet needs that settled with OpenAI first.
