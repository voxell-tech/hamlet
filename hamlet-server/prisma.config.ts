import { defineConfig } from "prisma/config";

// The app reaches D1 through @prisma/adapter-d1 and migrations are written
// with `prisma migrate diff` between schema files, so no database is ever
// opened here. The schema engine still insists on a datasource url.
export default defineConfig({
  schema: "prisma/schema.prisma",
  datasource: {
    url: "file:./.wrangler/prisma-unused.sqlite",
  },
});
