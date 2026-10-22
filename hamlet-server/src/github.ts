import { HttpError } from "./errors.ts";
import type { DevicePoll, DeviceStart, GitHub, GitHubUser } from "./services.ts";

const DEVICE_CODE_URL = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL = "https://github.com/login/oauth/access_token";
const USER_URL = "https://api.github.com/user";
const DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";
const USER_AGENT = "hamlet-server";

const POLL_ERRORS: Record<string, DevicePoll["status"]> = {
  authorization_pending: "pending",
  slow_down: "slow_down",
  expired_token: "expired",
  access_denied: "denied",
  incorrect_device_code: "bad_code",
};

/** The GitHub OAuth device flow; it needs only the client id, no secret. */
export function createGitHub(clientId: string | undefined, fetcher: typeof fetch = fetch): GitHub {
  const requireClientId = (): string => {
    if (!clientId)
      throw new HttpError(500, "internal", "GITHUB_CLIENT_ID is not set on the server");
    return clientId;
  };

  return {
    async startDevice(): Promise<DeviceStart> {
      const body = await postForm(fetcher, DEVICE_CODE_URL, {
        client_id: requireClientId(),
        scope: "read:user",
      });
      if (typeof body.device_code !== "string") throw githubFailure(body);
      return {
        deviceCode: body.device_code,
        userCode: String(body.user_code),
        verificationUri: String(body.verification_uri),
        interval: Number(body.interval ?? 5),
        expiresIn: Number(body.expires_in ?? 900),
      };
    },

    async pollDevice(deviceCode: string): Promise<DevicePoll> {
      const body = await postForm(fetcher, ACCESS_TOKEN_URL, {
        client_id: requireClientId(),
        device_code: deviceCode,
        grant_type: DEVICE_GRANT,
      });
      if (typeof body.access_token === "string") {
        return { status: "approved", accessToken: body.access_token };
      }
      const status = POLL_ERRORS[String(body.error)];
      if (!status || status === "approved") throw githubFailure(body);
      return { status };
    },

    async getUser(accessToken: string): Promise<GitHubUser> {
      const response = await fetcher(USER_URL, {
        headers: {
          Authorization: `Bearer ${accessToken}`,
          Accept: "application/vnd.github+json",
          "User-Agent": USER_AGENT,
          "X-GitHub-Api-Version": "2022-11-28",
        },
      });
      if (!response.ok) {
        throw new HttpError(500, "internal", `GitHub user lookup failed (${response.status})`);
      }
      const user = (await response.json()) as Record<string, unknown>;
      const login = String(user.login);
      return {
        id: String(user.id),
        login,
        name: typeof user.name === "string" && user.name ? user.name : login,
        avatarUrl: typeof user.avatar_url === "string" ? user.avatar_url : "",
      };
    },
  };
}

async function postForm(
  fetcher: typeof fetch,
  url: string,
  fields: Record<string, string>,
): Promise<Record<string, unknown>> {
  const response = await fetcher(url, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/x-www-form-urlencoded",
      "User-Agent": USER_AGENT,
    },
    body: new URLSearchParams(fields).toString(),
  });
  try {
    return (await response.json()) as Record<string, unknown>;
  } catch {
    throw new HttpError(500, "internal", `GitHub answered ${response.status} without JSON`);
  }
}

function githubFailure(body: Record<string, unknown>): HttpError {
  const detail = body.error_description ?? body.error ?? "unexpected answer";
  return new HttpError(500, "internal", `GitHub sign-in failed: ${String(detail)}`);
}
