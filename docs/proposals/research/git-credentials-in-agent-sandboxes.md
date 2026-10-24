<!-- Researched 2026-10-13 from vendor documentation. The claims about Claude Code on the web, the GitHub Copilot cloud agent and Cloudflare's git gateway were checked again against their pages. -->

# How sandboxed coding agents push to git without holding the credential

## 1. Patterns

**P1. A git proxy that knows the git protocol and swaps a scoped credential for the real one.** Git inside the sandbox talks to a proxy outside it. Git's credential in the sandbox is a "custom-built scoped credential" that only the proxy accepts. The proxy checks it, checks the git request, and attaches the real GitHub token before forwarding ([Anthropic engineering](https://www.anthropic.com/engineering/claude-code-sandboxing)).

- **Claude Code on the web** uses this. Real credentials "stay encrypted on Anthropic's servers and never enter a session's VM" ([docs](https://code.claude.com/docs/en/claude-code-on-the-web)).
  - The proxy rejects branch deletions and pushes of tags or other non-branch refs.
  - It serves API requests only for the repositories attached to the session.
  - It "doesn't limit which branches a push can update". For that, the docs send you to GitHub branch protection or rulesets ([GitHub proxy](https://code.claude.com/docs/en/cloud-environments#github-proxy)).
- **Claude self-hosted runners** can opt into the same proxy (`--use-anthropic-git-proxy`). Git then uses the creator's GitHub OAuth token for user sessions, or the org's GitHub App installation token for bot sessions, and the runner image holds no git credentials ([self-hosted deploy](https://code.claude.com/docs/en/self-hosted-environments-deploy#configure-git)).
- **Cloudflare Sandbox** documents the same pattern as a recipe.
  - `interceptOutboundHttps("github.com", GitGateway)` routes the container's git traffic to a Worker. The Worker allows only `git-upload-pack` for one repository, adds `Authorization`, and rejects everything else with 403.
  - "A `git push` from the sandbox fails with `403`" ([clone a private repository](https://developers.cloudflare.com/sandbox/network/clone-a-private-repository/)). Allowing pushes would mean also allowing `git-receive-pack` in that Worker.
  - The container must trust a Cloudflare container CA (`GIT_SSL_CAINFO`).

**P2. A platform tool pushes; the agent never runs `git push`.** The **GitHub Copilot cloud agent** "can only perform simple push operations. It cannot directly run `git push` or other Git commands" ([risks and mitigations](https://docs.github.com/en/copilot/concepts/agents/cloud-agent/risks-and-mitigations)).

- It pushes to a single branch: a new `copilot/` branch, or the existing PR's branch when triggered from a PR.
- It is subject to branch protections. It can't mark its PR ready for review, approve it, or merge it.
- Each session can work on one repository, one branch and one PR, for at most 59 minutes ([about the agent](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-coding-agent)).
- A firewall limits the agent's internet access, but always allows the hosts Copilot uses to reach GitHub ([firewall](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/customize-the-agent-firewall)).

This is the owner's rejected idea (a), and GitHub runs it at scale.

**P3. A generic egress proxy that injects headers, often TLS-terminating with a CA trusted in the sandbox.** This is the owner's rejected idea (b). Every platform that offers it presents it as a general API-key feature, not as their git mechanism.

- **Vercel Sandbox** "credentials brokering": `transform` rules inject headers, and "the secrets never enter the sandbox". The firewall terminates TLS with "a unique, per-sandbox CA".
  - Matchers can select on path, method, query and headers, but "never block a request".
  - Alternatively, `forwardURL` sends requests to your own proxy with a per-sandbox OIDC token ([firewall](https://vercel.com/docs/sandbox/concepts/firewall)).
- **Codex Cloud** "network secrets": programs see a placeholder, and the proxy substitutes the real value for allowed HTTPS destinations ([cloud environments](https://developers.openai.com/codex/environments/cloud-environments)).
- **Claude Code** network secrets work the same way, but explicitly never apply to GitHub, which goes through P1 instead ([cloud environments](https://code.claude.com/docs/en/cloud-environments#add-network-secrets)).
- **Modal** Sidecars can hold a secret and proxy the sandbox's HTTPS traffic. Reading or rewriting requests requires terminating TLS "using a certificate authority that the Sandbox trusts" ([sidecars](https://modal.com/docs/guide/sandbox-sidecars)).
- **Cloudflare** `outboundByHost` handlers inject headers ([outbound traffic](https://developers.cloudflare.com/sandbox/sdk/guides/outbound-traffic/)). Cloudflare's `credentialProxy` is only a `mountBucket` option for S3 credentials; it has nothing to do with git ([mount buckets](https://developers.cloudflare.com/sandbox/sdk/guides/mount-buckets/)).
- **Anthropic's `srt`** (bubblewrap/Seatbelt) has a `credentials` `mode: "mask"`. It puts a sentinel in the sandbox and substitutes the real value on egress, which requires `network.tlsTerminate` (a MITM CA) ([sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime)).

**P4. The token goes into the sandbox, scoped and short-lived.** This does not meet Hamlet's requirement.

- **E2B** says to pass the token through `envs` into `git clone https://x-access-token:$GITHUB_TOKEN@…` ([E2B](https://docs.e2b.dev/migration/git-deprecation)).
- **Daytona** passes tokens per operation and has an endpoint that "stores credentials in plaintext on disk" ([Daytona](https://www.daytona.io/docs/en/git-operations/)).
- **Claude self-hosted** runners without the proxy: Anthropic recommends minting "a short-lived, least-scoped token per session" ([self-hosted](https://code.claude.com/docs/en/self-hosted-environments-deploy#configure-git)).

**P5. Platform-created pull requests (handoff).**

- **Codex** shows a diff, and you "commit or open a pull request when you're ready". Legacy Codex secrets "are removed before the agent phase starts" ([Codex](https://developers.openai.com/codex/cloud), [legacy](https://developers.openai.com/codex/cloud/environments)).
- **Cursor** agents "push changes to your repo" and open draft PRs through the Cursor GitHub/GitLab App ([Cursor](https://cursor.com/docs/cloud-agent), [security](https://cursor.com/docs/cloud-agent/security)).

**Supporting mechanisms**

- **GitHub App installation tokens:**
  - They expire after 1 hour.
  - They can be narrowed to listed repositories (up to 500) and to a subset of permissions.
  - They can never exceed what the installation was granted. The endpoint has no branch parameter ([GitHub](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app)).
- **Fine-grained PATs** can be limited to selected repositories, with an expiry ([GitHub](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)).
- **GitHub rulesets** limit branches. "Restrict creations/updates/deletions" apply by name pattern, and only bypass actors (roles, teams, GitHub Apps) are exempt ([rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)).
- **Git's own mechanisms:**
  - Credential helpers are programs that git runs to get credentials ([gitcredentials](https://git-scm.com/docs/gitcredentials)).
  - `http.extraHeader` adds headers and can be set per URL via `http.<url>.*` ([git-config](https://git-scm.com/docs/git-config)).
  - Both are configured and run in the same place git runs. So if they are set inside the sandbox, the agent can read or invoke them (my inference from those docs). They belong on the proxy side.

## 2. Table

| Product | Pattern | Where the credential lives | What the agent can push to |
|---|---|---|---|
| Claude Code on the web | P1 | Anthropic servers; VM has a scoped credential | Branches of the session's repositories; branch names not limited (use rulesets) |
| Claude self-hosted | P1 (opt-in) or P4 | Anthropic, or your runner | Same as above, or whatever your token allows |
| Cloudflare Sandbox | P1 recipe / P3 | Worker env | Whatever your Worker allows (the example allows fetch only) |
| GitHub Copilot cloud agent | P2 | Platform (unconfirmed) | One `copilot/` or PR branch, through a tool |
| Codex Cloud | P5 (+P3 for APIs) | Unconfirmed for GitHub | Unconfirmed |
| Cursor Cloud Agents | P5 | Unconfirmed | Its own branch, then a draft PR |
| Devin | GitHub App (org-level permissions) | Unconfirmed | Branches/PRs; org-level permissions ([Devin](https://docs.devin.ai/integrations/gh)) |
| Vercel Sandbox | P3 | Vercel firewall | Not git-aware |
| Modal | P3 (sidecar) | Sidecar container | Whatever your sidecar allows |
| E2B, Daytona | P4 | Inside the sandbox | Whatever the token allows |

## 3. What scales

**P1 holds up best.**

- One stateless service holds the real tokens.
- Sandboxes, local or remote, need only reach that service.
- Policy (repo, ref prefix, no deletes, no tags) is enforced per request, from a table keyed by the scoped credential.
- The agent keeps plain `git`, and no other traffic is decrypted.
- Anthropic runs it as the default for every hosted session, and offers it to self-hosted runners.

**The other patterns fall short:**

- **P3** can't see ref names: Vercel's matchers cover path, method, query and headers, not the body. It also needs a CA in every sandbox, and is still per-host header injection.
- **P2** works, but every git verb becomes a product feature.
- **P4** leaks by design.

**Per-user scale:** pair P1 with a GitHub App that mints a fresh repo-scoped `contents:write` token per push, valid for 1 hour, so no long-lived user token sits behind the proxy. Add rulesets as a server-side backstop.

## 4. Recommendation for Hamlet

Build a **git smart-HTTP proxy in the agent server (P1)**. It is neither rejected design:

- It is not idea (a): agents run ordinary `git push`.
- It is not idea (b): there is no MITM, no CA in the sandbox, and it handles only git endpoints.

**What it takes:**

1. **Proxy routes.**
   - Accept only `GET …/info/refs?service=git-upload-pack|git-receive-pack`, `POST …/git-upload-pack` and `POST …/git-receive-pack`, for the hamlet's single repository. This mirrors Cloudflare's `GitGateway`.
   - For receive-pack, parse the pkt-line command list before the pack. Reject zero-SHA deletions, anything outside `refs/heads/<hamlet-prefix>/*`, and tags.
   - Stream the rest upstream with `Authorization` added, and pass through the `Git-Protocol` header.
2. **Sandbox side.**
   - The clone's `origin` points at the proxy, e.g. `http://git.hamlet.internal/<hamlet>/<repo>.git`.
   - The sandbox's git sends a per-hamlet random credential, via the URL or `http.extraHeader`. Agents may read it: it is useless anywhere but the proxy, and it maps to exactly one repo and one branch prefix.
   - Locally, reach the proxy the way the sandbox already reaches its egress proxy (a Unix socket under bubblewrap, a localhost port under Seatbelt).
   - Remotely, the same proxy is an HTTPS endpoint. Make the credential short-lived and, where the platform offers it, bind it to the sandbox's identity (Vercel's `forwardURL` OIDC token, Cloudflare's `ctx.containerId`).
3. **Token storage.**
   - The user's token stays in the server.
   - Later, support a GitHub App and mint a 1-hour installation token per push, scoped to the hamlet's repository.
   - Non-GitHub hosts keep the stored PAT behind the same proxy.
4. **Backstops.**
   - Recommend a GitHub ruleset that restricts updates to the default branch and blocks force pushes.
   - Block direct sandbox access to the git host, so the proxy is the only path.

## 5. Unconfirmed

- **Claude Code's branch limits:** Anthropic's blog says the proxy ensures "it is only pushing to the configured branch". Current docs say the proxy doesn't limit branches. I couldn't confirm which is current behaviour beyond the docs. I also couldn't find the format of the scoped credential.
- **Copilot:** which token, if any, exists inside its environment, and how its push tool authenticates.
- **Codex Cloud:** whether any GitHub credential is in the VM, and whether a CA is installed for network secrets.
- **Cursor and Devin:** whether git credentials are inside the VM. Their docs don't say. Cursor's "git egress proxy" is documented only for IP allowlisting.
- **Daytona:** whether a per-operation token passes through processes inside the sandbox.
- **Claude network secrets:** whether they rely on a CA trusted inside the VM.
- **Git over a Unix socket:** whether git can reach a proxy directly on a Unix socket without a bridge. I didn't verify this.
- **Missing source:** web search was partly unavailable. Every claim above comes from fetched vendor pages.
