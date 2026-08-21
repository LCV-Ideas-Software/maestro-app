import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = process.cwd();
const snapshotPath = resolve(root, "src/editor/posteditor/parity-snapshot.json");
const snapshot = JSON.parse(await readFile(snapshotPath, "utf8"));
const failures = [];
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const normalizedTextSha256 = (text) => sha256(text.replace(/\r\n/g, "\n"));
const isSha256 = (value) => /^[a-f0-9]{64}$/.test(value);

if (snapshot.schema_version !== "posteditor_parity_snapshot.v1") {
  failures.push("schema_version must be posteditor_parity_snapshot.v1");
}
if (!/^mainsite_post_html\.v\d+$/.test(snapshot.sanitizer_profile ?? "")) {
  failures.push("sanitizer_profile must be a versioned mainsite_post_html profile");
}
for (const [label, commit] of [
  ["admin_commit", snapshot.admin_commit],
  ["mainsite_commit", snapshot.mainsite_commit],
]) {
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) failures.push(`${label} is not a full commit SHA`);
}

for (const group of ["admin_source_files", "mainsite_renderer_files"]) {
  const entries = snapshot[group] ?? {};
  if (Object.keys(entries).length === 0) failures.push(`${group} is empty`);
  for (const [file, expected] of Object.entries(entries)) {
    if (!isSha256(expected)) failures.push(`${group}:${file} has an invalid SHA-256`);
  }
}

const localFiles = snapshot.maestro_files ?? {};
if (Object.keys(localFiles).length === 0) failures.push("maestro_files is empty");
for (const [file, expected] of Object.entries(localFiles)) {
  try {
    const actual = normalizedTextSha256(await readFile(resolve(root, file), "utf8"));
    if (actual !== expected) failures.push(`${file} drifted: expected ${expected}, got ${actual}`);
  } catch (error) {
    failures.push(`${file} could not be read: ${error instanceof Error ? error.message : error}`);
  }
}

const profileFiles = ["src/services/editorial.ts", "src-tauri/src/mainsite_draft.rs"];
for (const file of profileFiles) {
  const source = await readFile(resolve(root, file), "utf8");
  if (!source.includes(snapshot.sanitizer_profile)) {
    failures.push(`${file} does not declare sanitizer profile ${snapshot.sanitizer_profile}`);
  }
}

if (failures.length > 0) {
  console.error("PostEditor parity snapshot verification failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  console.error("Refresh the snapshot only after reviewing admin-app and mainsite-app drift.");
  process.exit(1);
}

console.log(
  `PostEditor parity snapshot verified (${Object.keys(localFiles).length} Maestro files; admin ${snapshot.admin_commit.slice(0, 12)}; MainSite ${snapshot.mainsite_commit.slice(0, 12)}).`,
);
