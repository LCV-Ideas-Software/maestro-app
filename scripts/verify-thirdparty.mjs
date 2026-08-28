import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const INVENTORY = "THIRDPARTY.md";
const NODE_HEADER = ["Component", "Version", "License", "Scope", "Source"];

function normalizeCell(value) {
  return value.trim().replace(/^`|`$/gu, "");
}

function parseTable(markdown, heading, label) {
  const lines = markdown.split(/\r?\n/u);
  const headingIndex = lines.findIndex((line) => line.trim() === heading);
  assert.notEqual(headingIndex, -1, `${label} heading is missing`);

  const nextHeadingIndex = lines.findIndex(
    (line, index) => index > headingIndex && /^#{1,3}\s/u.test(line),
  );
  const boundary = nextHeadingIndex === -1 ? lines.length : nextHeadingIndex;
  const headerIndex = lines.findIndex(
    (line, index) =>
      index > headingIndex &&
      index < boundary &&
      line.trim() === `| ${NODE_HEADER.join(" | ")} |`,
  );
  assert.notEqual(headerIndex, -1, `${label} table header is missing`);

  const rows = [];
  for (const line of lines.slice(headerIndex + 2, boundary)) {
    if (!line.startsWith("|")) break;
    const cells = line.split("|").slice(1, -1).map(normalizeCell);
    assert.equal(
      cells.length,
      NODE_HEADER.length,
      `invalid ${label} row: ${line}`,
    );
    const [name, version, license, scope, source] = cells;
    rows.push({ name, version, license, scope, source });
  }
  assert.ok(rows.length > 0, `${label} table is empty`);
  return rows;
}

function assertUniqueNames(rows, label) {
  const names = rows.map(({ name }) => name);
  assert.equal(
    new Set(names).size,
    names.length,
    `${label} contains duplicate components`,
  );
}

function expectedNodeRows(packageJson, packageLock) {
  for (const unsupported of ["optionalDependencies", "peerDependencies"]) {
    assert.deepEqual(
      packageJson[unsupported] ?? {},
      {},
      `${unsupported} must be added explicitly to the THIRDPARTY contract before use`,
    );
  }

  const scopes = [
    ["dependencies", "runtime"],
    ["devDependencies", "development"],
  ];
  const rows = [];

  for (const [manifestKey, scope] of scopes) {
    const declaredDependencies = packageJson[manifestKey] ?? {};
    assert.deepEqual(
      packageLock.packages?.[""]?.[manifestKey] ?? {},
      declaredDependencies,
      `${manifestKey} differs between package.json and package-lock.json`,
    );
    const dependencies = Object.entries(declaredDependencies).sort(
      ([left], [right]) => (left < right ? -1 : left > right ? 1 : 0),
    );
    for (const [name, requirement] of dependencies) {
      assert.ok(
        !requirement.startsWith("npm:"),
        `${name} uses an npm alias and needs an explicit inventory source contract`,
      );
      const lockEntry = packageLock.packages?.[`node_modules/${name}`];
      assert.ok(lockEntry, `${name} is missing from package-lock.json`);
      assert.equal(
        typeof lockEntry.version,
        "string",
        `${name} lacks a locked version`,
      );
      assert.equal(
        typeof lockEntry.license,
        "string",
        `${name} lacks lockfile license metadata`,
      );
      assert.ok(
        lockEntry.license.trim(),
        `${name} has an empty lockfile license`,
      );
      assert.equal(
        lockEntry.link,
        undefined,
        `${name} is a linked dependency and needs an explicit inventory source contract`,
      );
      assert.match(
        lockEntry.resolved ?? "",
        /^https:\/\/registry\.npmjs\.org\//u,
        `${name} is not resolved from the npmjs registry`,
      );
      assert.ok(
        !lockEntry.name || lockEntry.name === name,
        `${name} resolves as the npm alias ${lockEntry.name}`,
      );
      rows.push({
        name,
        version: lockEntry.version,
        license: lockEntry.license,
        scope,
        source: `https://www.npmjs.com/package/${name}`,
      });
    }
  }
  return rows;
}

function parseCargoManifest(cargoToml) {
  const rows = [];
  let scope = null;

  for (const sourceLine of cargoToml.split(/\r?\n/u)) {
    const line = sourceLine.trim();
    if (line === "[build-dependencies]") {
      scope = "build";
      continue;
    }
    if (line === "[dependencies]") {
      scope = "runtime";
      continue;
    }
    if (line.startsWith("[")) {
      assert.ok(
        !/dependencies/u.test(line),
        `unsupported Cargo dependency section: ${line}`,
      );
      scope = null;
      continue;
    }
    if (!scope || !line || line.startsWith("#")) continue;

    const declaration = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/u);
    assert.ok(
      declaration,
      `unsupported Cargo dependency declaration: ${sourceLine}`,
    );
    const [, name, value] = declaration;
    const version =
      value.match(/^"([^"]+)"/u)?.[1] ??
      value.match(/\bversion\s*=\s*"([^"]+)"/u)?.[1];
    assert.ok(version, `${name} lacks an explicit Cargo version requirement`);
    rows.push({ name, requirement: version, scope });
  }
  return rows;
}

function parseCargoLock(cargoLock) {
  return cargoLock
    .split(/^\[\[package\]\]\s*$/mu)
    .slice(1)
    .map((block) => ({
      name: block.match(/^name\s*=\s*"([^"]+)"\s*$/mu)?.[1],
      version: block.match(/^version\s*=\s*"([^"]+)"\s*$/mu)?.[1],
      source: block.match(/^source\s*=\s*"([^"]+)"\s*$/mu)?.[1],
    }))
    .filter(({ name, version }) => name && version);
}

function numericVersion(version, label) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:\+.*)?$/u);
  assert.ok(match, `unsupported ${label} version: ${version}`);
  return match.slice(1).map(Number);
}

function cargoRequirementMatches(requirement, version) {
  const cleaned = requirement.replace(/^\^/u, "");
  assert.match(
    cleaned,
    /^\d+(?:\.\d+){0,2}$/u,
    `unsupported Cargo requirement: ${requirement}`,
  );
  const parts = cleaned.split(".").map(Number);
  const [major, minor = 0, patch = 0] = parts;
  const [candidateMajor, candidateMinor, candidatePatch] = numericVersion(
    version,
    "Cargo lock",
  );

  const atOrAboveFloor =
    candidateMajor > major ||
    (candidateMajor === major && candidateMinor > minor) ||
    (candidateMajor === major &&
      candidateMinor === minor &&
      candidatePatch >= patch);
  if (!atOrAboveFloor) return false;
  if (parts.length === 1) return candidateMajor === major;
  if (major > 0) return candidateMajor === major;
  if (parts.length === 2) {
    return candidateMajor === 0 && candidateMinor === minor;
  }
  if (minor > 0) return candidateMajor === 0 && candidateMinor === minor;
  return (
    candidateMajor === 0 && candidateMinor === 0 && candidatePatch === patch
  );
}

function normalizedCargoLockSha256(cargoLock) {
  return createHash("sha256")
    .update(cargoLock.replace(/\r\n/gu, "\n"))
    .digest("hex");
}

function documentedCargoLockSha256(inventory) {
  const matches = [
    ...inventory.matchAll(
      /^Normalized `Cargo\.lock` SHA-256: `([0-9a-f]{64})`$/gmu,
    ),
  ];
  assert.equal(
    matches.length,
    1,
    "THIRDPARTY must contain exactly one normalized Cargo.lock SHA-256",
  );
  return matches[0][1];
}

function expectedRustRows(cargoToml, cargoLock) {
  const locked = parseCargoLock(cargoLock);
  return parseCargoManifest(cargoToml).map(({ name, requirement, scope }) => {
    const candidates = locked.filter(
      (entry) =>
        entry.name === name &&
        cargoRequirementMatches(requirement, entry.version),
    );
    assert.equal(
      candidates.length,
      1,
      `${name} must resolve to exactly one matching package in Cargo.lock`,
    );
    const [{ version, source }] = candidates;
    assert.equal(
      source,
      "registry+https://github.com/rust-lang/crates.io-index",
      `${name} is not resolved from the crates.io registry`,
    );
    return {
      name,
      version,
      scope,
      source: `https://crates.io/crates/${name}/${version}`,
    };
  });
}

export function verifyThirdPartyInventory({
  packageJson,
  packageLock,
  cargoToml,
  cargoLock,
  inventory,
}) {
  assert.equal(
    documentedCargoLockSha256(inventory),
    normalizedCargoLockSha256(cargoLock),
    "the transitive Rust review does not match the current Cargo.lock",
  );

  const nodeRows = parseTable(
    inventory,
    "# Third-Party Components",
    "Node inventory",
  );
  assertUniqueNames(nodeRows, "Node inventory");
  assert.deepEqual(
    nodeRows,
    expectedNodeRows(packageJson, packageLock),
    "Node inventory does not match direct manifest and lockfile metadata",
  );

  const rustRows = parseTable(
    inventory,
    "## Rust components",
    "Rust inventory",
  );
  assertUniqueNames(rustRows, "Rust inventory");
  for (const row of rustRows) {
    assert.ok(row.license, `${row.name} has an empty Rust license expression`);
  }
  assert.deepEqual(
    rustRows.map(({ license: _license, ...row }) => row),
    expectedRustRows(cargoToml, cargoLock),
    "Rust inventory does not match direct manifest and lockfile metadata",
  );
}

async function main() {
  const root = process.cwd();
  const [packageJson, packageLock, cargoToml, cargoLock, inventory] =
    await Promise.all([
      readFile(resolve(root, "package.json"), "utf8").then(JSON.parse),
      readFile(resolve(root, "package-lock.json"), "utf8").then(JSON.parse),
      readFile(resolve(root, "src-tauri/Cargo.toml"), "utf8"),
      readFile(resolve(root, "src-tauri/Cargo.lock"), "utf8"),
      readFile(resolve(root, INVENTORY), "utf8"),
    ]);
  verifyThirdPartyInventory({
    packageJson,
    packageLock,
    cargoToml,
    cargoLock,
    inventory,
  });
  console.log(
    "THIRDPARTY inventory matches all direct Node and Rust dependencies.",
  );
}

const entryPoint = process.argv[1]
  ? pathToFileURL(resolve(process.argv[1])).href
  : undefined;
if (entryPoint === import.meta.url) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
