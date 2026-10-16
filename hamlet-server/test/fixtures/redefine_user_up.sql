-- RedefineTables
PRAGMA defer_foreign_keys=ON;
PRAGMA foreign_keys=OFF;
CREATE TABLE "new_User" (
    "id" TEXT NOT NULL PRIMARY KEY,
    "githubId" TEXT NOT NULL,
    "login" TEXT NOT NULL,
    "bio" TEXT NOT NULL DEFAULT '',
    "name" TEXT NOT NULL,
    "avatarUrl" TEXT NOT NULL,
    "stripeAccountId" TEXT,
    "payoutsEnabled" BOOLEAN NOT NULL DEFAULT false,
    "createdAt" DATETIME NOT NULL,
    "updatedAt" DATETIME NOT NULL
);
INSERT INTO "new_User" ("avatarUrl", "createdAt", "githubId", "id", "login", "name", "payoutsEnabled", "stripeAccountId", "updatedAt") SELECT "avatarUrl", "createdAt", "githubId", "id", "login", "name", "payoutsEnabled", "stripeAccountId", "updatedAt" FROM "User";
DROP TABLE "User";
ALTER TABLE "new_User" RENAME TO "User";
CREATE UNIQUE INDEX "User_githubId_key" ON "User"("githubId");
CREATE UNIQUE INDEX "User_stripeAccountId_key" ON "User"("stripeAccountId");
CREATE INDEX "User_login_idx" ON "User"("login");
PRAGMA foreign_keys=ON;
PRAGMA defer_foreign_keys=OFF;

