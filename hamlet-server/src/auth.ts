import type { RequestHandler, Response } from "express";
import type { PrismaClient, User } from "./generated/prisma/client.ts";
import { unauthorized } from "./errors.ts";
import { newId, newSessionToken, sha256Hex } from "./ids.ts";
import type { Services } from "./services.ts";

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Locals {
      user?: User;
      tokenHash?: string;
    }
  }
}

const SESSION_DAYS = 90;
const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Looks up the bearer token when one is sent. A missing, unknown or
 * expired token leaves the request anonymous; `requireUser` turns that
 * into a 401 on the routes that need a user.
 */
export function authenticate(services: Services): RequestHandler {
  return async (req, res, next) => {
    const token = bearerToken(req.headers.authorization);
    if (token) {
      const tokenHash = sha256Hex(token);
      const session = await services.prisma.session.findUnique({
        where: { tokenHash },
        include: { user: true },
      });
      if (session && session.expiresAt > services.now()) {
        res.locals.user = session.user;
        res.locals.tokenHash = tokenHash;
      }
    }
    next();
  };
}

export const requireAuth: RequestHandler = (_req, res, next) => {
  if (!res.locals.user) throw unauthorized();
  next();
};

export function currentUser(res: Response): User {
  const user = res.locals.user;
  if (!user) throw unauthorized();
  return user;
}

export function maybeUser(res: Response): User | undefined {
  return res.locals.user;
}

/** Makes a session and returns its token; only the token's hash is kept. */
export async function createSession(
  prisma: PrismaClient,
  userId: string,
  now: Date,
): Promise<string> {
  const token = newSessionToken();
  await prisma.session.create({
    data: {
      id: newId("s"),
      tokenHash: sha256Hex(token),
      userId,
      createdAt: now,
      expiresAt: new Date(now.getTime() + SESSION_DAYS * DAY_MS),
    },
  });
  return token;
}

function bearerToken(header: string | undefined): string | null {
  const match = header?.match(/^Bearer\s+(\S+)\s*$/i);
  return match ? match[1]! : null;
}
