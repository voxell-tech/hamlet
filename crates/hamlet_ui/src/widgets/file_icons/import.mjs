// Bundles Catppuccin's VS Code file icons into icons.json beside this
// script: which icon each file and folder name takes, and each icon's
// SVG in the mocha flavour.
//
//   git clone https://github.com/catppuccin/vscode-icons <checkout>
//   node import.mjs <checkout>
//
// Copy the checkout's LICENSE here too when it changes.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const checkout = process.argv[2];
if (!checkout) {
  console.error("usage: node import.mjs <catppuccin/vscode-icons checkout>");
  process.exit(1);
}
const files = await import(join(checkout, "src/defaults/fileIcons.ts"));
const folders = await import(join(checkout, "src/defaults/folderIcons.ts"));
const lower = (table) =>
  Object.fromEntries(
    Object.entries(table).map(([name, icon]) => [name.toLowerCase(), icon]),
  );
const names = lower(files.fileNames);
const extensions = lower(files.fileExtensions);
const folderNames = lower(folders.folderNames);

const wanted = new Set(["_file", "_folder", "_folder_open"]);
for (const icon of [...Object.values(names), ...Object.values(extensions)]) {
  wanted.add(icon);
}
for (const icon of Object.values(folderNames)) {
  wanted.add(icon);
  wanted.add(`${icon}_open`);
}
const icons = {};
for (const icon of [...wanted].sort()) {
  const svg = readFileSync(join(checkout, "icons/mocha", `${icon}.svg`), "utf8");
  icons[icon] = svg.replace(/\s*\n\s*/g, "");
}

const source = execFileSync("git", ["-C", checkout, "rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
const bundle = {
  source: `catppuccin/vscode-icons@${source}`,
  names,
  extensions,
  folders: folderNames,
  icons,
};
writeFileSync(
  new URL("icons.json", import.meta.url),
  `${JSON.stringify(bundle, null, 1)}\n`,
);
console.log(`${Object.keys(icons).length} icons from ${bundle.source}`);
