-- CreateTable
CREATE TABLE "Bundle" (
    "id" TEXT NOT NULL PRIMARY KEY,
    "ownerId" TEXT NOT NULL,
    CONSTRAINT "Bundle_ownerId_fkey" FOREIGN KEY ("ownerId") REFERENCES "User" ("id") ON DELETE CASCADE ON UPDATE CASCADE
);

-- RedefineTables
PRAGMA defer_foreign_keys=ON;
PRAGMA foreign_keys=OFF;
CREATE TABLE "new_Listing" (
    "id" TEXT NOT NULL PRIMARY KEY,
    "kind" TEXT NOT NULL,
    "title" TEXT NOT NULL,
    "summary" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "category" TEXT NOT NULL,
    "tags" TEXT NOT NULL,
    "priceCents" INTEGER NOT NULL,
    "status" TEXT NOT NULL,
    "sellerId" TEXT NOT NULL,
    "coverKey" TEXT,
    "coverType" TEXT,
    "coverVersion" INTEGER NOT NULL DEFAULT 0,
    "removedReason" TEXT,
    "featured" BOOLEAN NOT NULL DEFAULT false,
    "ownersCount" INTEGER NOT NULL DEFAULT 0,
    "reviewsCount" INTEGER NOT NULL DEFAULT 0,
    "starsAvg" REAL NOT NULL DEFAULT 0,
    "createdAt" DATETIME NOT NULL,
    "updatedAt" DATETIME NOT NULL,
    CONSTRAINT "Listing_sellerId_fkey" FOREIGN KEY ("sellerId") REFERENCES "User" ("id") ON DELETE CASCADE ON UPDATE CASCADE
);
INSERT INTO "new_Listing" ("category", "coverKey", "coverType", "coverVersion", "createdAt", "description", "id", "kind", "ownersCount", "priceCents", "removedReason", "reviewsCount", "sellerId", "starsAvg", "status", "summary", "tags", "title", "updatedAt") SELECT "category", "coverKey", "coverType", "coverVersion", "createdAt", "description", "id", "kind", "ownersCount", "priceCents", "removedReason", "reviewsCount", "sellerId", "starsAvg", "status", "summary", "tags", "title", "updatedAt" FROM "Listing";
DROP TABLE "Listing";
ALTER TABLE "new_Listing" RENAME TO "Listing";
CREATE INDEX "Listing_status_kind_idx" ON "Listing"("status", "kind");
CREATE INDEX "Listing_sellerId_createdAt_idx" ON "Listing"("sellerId", "createdAt");
PRAGMA foreign_keys=ON;
PRAGMA defer_foreign_keys=OFF;

