import { Router } from "express";
import { z } from "zod";
import { createSession, requireAuth } from "../auth.ts";
import { HttpError, badRequest } from "../errors.ts";
import { newId } from "../ids.ts";
import type { DevicePoll, GitHubUser, Services } from "../services.ts";
import { userJson } from "../views.ts";

const pollSchema = z.object({ deviceCode: z.string().min(1).max(200) });

export function authRoutes(services: Services): Router {
  const router = Router();

  router.post("/device", async (_req, res) => {
    res.json(await services.github.startDevice());
  });

  router.post("/device/poll", async (req, res) => {
    const { deviceCode } = pollSchema.parse(req.body);
    const poll = await services.github.pollDevice(deviceCode);
    if (poll.status !== "approved") throw pollError(poll.status);
    const githubUser = await services.github.getUser(poll.accessToken);
    const user = await upsertUser(services, githubUser);
    const token = await createSession(services.prisma, user.id, services.now());
    res.json({ token, user: userJson(user) });
  });

  router.delete("/session", requireAuth, async (_req, res) => {
    const tokenHash = res.locals.tokenHash;
    if (tokenHash) await services.prisma.session.deleteMany({ where: { tokenHash } });
    res.status(204).end();
  });

  return router;
}

function pollError(status: Exclude<DevicePoll["status"], "approved">): HttpError {
  switch (status) {
    case "pending":
      return new HttpError(428, "pending", "not approved on GitHub yet");
    case "slow_down":
      return new HttpError(429, "slow_down", "polling too fast; wait 5 more seconds");
    case "expired":
      return new HttpError(410, "expired", "the code expired; start again");
    case "denied":
      return new HttpError(403, "denied", "sign-in was denied on GitHub");
    case "bad_code":
      return badRequest("unknown device code");
  }
}

async function upsertUser(services: Services, githubUser: GitHubUser) {
  const now = services.now();
  const profile = {
    login: githubUser.login,
    name: githubUser.name,
    avatarUrl: githubUser.avatarUrl,
    updatedAt: now,
  };
  return services.prisma.user.upsert({
    where: { githubId: githubUser.id },
    update: profile,
    create: { id: newId("u"), githubId: githubUser.id, createdAt: now, ...profile },
  });
}
