---
status: accepted, amends [0006](0006-subscription-logins-only-where-the-provider-allows-them.md)
---

# ChatGPT signs in through Codex's login

ADR 0006 had a chief's ChatGPT plan reached through "Sign in with
ChatGPT", the sign-in OpenAI offers open-source apps. That sign-in tells
an app nothing of the plan's use: OpenAI's usage address refuses its
token, its answers carry no limits, and its refusal at a limit names no
window and no reset (openai/codex#54219, open with no answer from OpenAI).
A chief running agents on a plan needs to see how much of it is left,
including the five-hour session limit.

So pi's provider `chatgpt` is pi-ai's `openai-codex` provider: the chief
signs in through Codex's own login, in the browser, and pi calls Codex's
address with that token. The same token reads the plan's use from the
address Codex reads, `chatgpt.com/backend-api/wham/usage`, and each answer
tells the plan's windows in its `x-codex-*` headers. Nothing is installed:
pi-ai carries the login and the calls.

## Consequences

- Hamlet still labels the action "Continue with ChatGPT", and the chief
  signs in once.
- The login is Codex's, not one OpenAI offers apps by name. OpenAI may
  close it to other programs; then ChatGPT goes back to "Sign in with
  ChatGPT", with no limits shown, until OpenAI offers a way to read them.
- A chief signed in through "Sign in with ChatGPT" before signs in again.
