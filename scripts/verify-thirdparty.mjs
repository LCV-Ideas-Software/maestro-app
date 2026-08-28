import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const INVENTORY = "THIRDPARTY.md";
const COMPONENT_HEADER = [
  "Component",
  "Version",
  "License",
  "Scope",
  "Modified?",
  "Source",
];
const TRACKED_DEPENDENCY_FILES = [
  "package-lock.json",
  "package.json",
  "src-tauri/Cargo.lock",
  "src-tauri/Cargo.toml",
];

// A locally patched or vendored component must be declared here with the exact
// text expected in the Modified? column. Absence means the locked upstream
// artifact is consumed unchanged.
const MODIFICATION_OVERRIDES = new Map();

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
      line.trim() === `| ${COMPONENT_HEADER.join(" | ")} |`,
  );
  assert.notEqual(headerIndex, -1, `${label} table header is missing`);
  assert.ok(
    lines
      .slice(headingIndex + 1, headerIndex)
      .every((line) => !line.trim().startsWith("|")),
    `${label} contains an unexpected table before its canonical header`,
  );
  assert.equal(
    lines[headerIndex + 1]?.trim(),
    `| ${COMPONENT_HEADER.map(() => "---").join(" | ")} |`,
    `${label} table separator is invalid`,
  );

  const rows = [];
  for (const line of lines.slice(headerIndex + 2, boundary)) {
    const trimmedLine = line.trim();
    if (!trimmedLine.startsWith("|")) continue;
    const cells = trimmedLine.split("|").slice(1, -1).map(normalizeCell);
    assert.equal(
      cells.length,
      COMPONENT_HEADER.length,
      `invalid ${label} row: ${line}`,
    );
    const [name, version, license, scope, modified, source] = cells;
    rows.push({ name, version, license, scope, modified, source });
  }
  assert.ok(rows.length > 0, `${label} table is empty`);
  return rows;
}

function modificationStatus(ecosystem, name, version) {
  return (
    MODIFICATION_OVERRIDES.get(`${ecosystem}:${name}@${version}`) ??
    MODIFICATION_OVERRIDES.get(`${ecosystem}:${name}`) ??
    "No"
  );
}

function assertUniqueComponents(rows, label) {
  const components = rows.map(
    ({ name, version, scope }) => `${name}\0${version}\0${scope}`,
  );
  assert.equal(
    new Set(components).size,
    components.length,
    `${label} contains duplicate components`,
  );
}

function expectedNodeRows(packageJson, packageLock) {
  const optionalDependencyNames = new Set(
    Object.keys(packageJson.optionalDependencies ?? {}),
  );
  const scopes = [
    ["dependencies", "runtime"],
    ["devDependencies", "development"],
    ["optionalDependencies", "optional"],
    ["peerDependencies", "peer"],
  ];
  const rows = [];
  const peerDependenciesMeta = packageJson.peerDependenciesMeta ?? {};
  assert.deepEqual(
    packageLock.packages?.[""]?.peerDependenciesMeta ?? {},
    peerDependenciesMeta,
    "peerDependenciesMeta differs between package.json and package-lock.json",
  );
  for (const [name, metadata] of Object.entries(peerDependenciesMeta)) {
    assert.ok(
      Object.hasOwn(packageJson.peerDependencies ?? {}, name),
      `${name} has peerDependenciesMeta without a peerDependency`,
    );
    assert.deepEqual(
      Object.keys(metadata).sort(),
      ["optional"],
      `${name} has unsupported peerDependenciesMeta`,
    );
    assert.equal(
      typeof metadata.optional,
      "boolean",
      `${name} peerDependenciesMeta.optional must be boolean`,
    );
  }

  for (const [manifestKey, scope] of scopes) {
    const declaredDependencies = packageJson[manifestKey] ?? {};
    assert.deepEqual(
      packageLock.packages?.[""]?.[manifestKey] ?? {},
      declaredDependencies,
      `${manifestKey} differs between package.json and package-lock.json`,
    );
    // npm gives optionalDependencies precedence over dependencies when the
    // same package name appears in both sections. Inventory that package once,
    // with its effective optional scope, while still validating both manifest
    // sections against the root package-lock entry above.
    const effectiveDependencies =
      manifestKey === "dependencies"
        ? Object.fromEntries(
            Object.entries(declaredDependencies).filter(
              ([name]) => !optionalDependencyNames.has(name),
            ),
          )
        : declaredDependencies;
    const dependencies = Object.entries(effectiveDependencies).sort(
      ([left], [right]) => (left < right ? -1 : left > right ? 1 : 0),
    );
    for (const [name, requirement] of dependencies) {
      assert.ok(
        !requirement.startsWith("npm:"),
        `${name} uses an npm alias and needs an explicit inventory source contract`,
      );
      const lockEntry = packageLock.packages?.[`node_modules/${name}`];
      if (
        scope === "peer" &&
        peerDependenciesMeta[name]?.optional === true &&
        !lockEntry
      ) {
        continue;
      }
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
        modified: modificationStatus("npm", name, lockEntry.version),
        source: `https://www.npmjs.com/package/${name}`,
      });
    }
  }
  return rows;
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

function expectedRustRows(cargoMetadata) {
  assert.equal(cargoMetadata.version, 1, "unsupported cargo metadata format");
  assert.ok(cargoMetadata.resolve?.root, "cargo metadata root is missing");
  const packagesById = new Map(
    cargoMetadata.packages.map((packageMetadata) => [
      packageMetadata.id,
      packageMetadata,
    ]),
  );
  assert.equal(
    packagesById.size,
    cargoMetadata.packages.length,
    "cargo metadata contains duplicate package IDs",
  );
  const rootNodes = cargoMetadata.resolve.nodes.filter(
    ({ id }) => id === cargoMetadata.resolve.root,
  );
  assert.equal(rootNodes.length, 1, "cargo metadata root node is ambiguous");

  const rows = [];
  for (const dependency of rootNodes[0].deps) {
    const packageMetadata = packagesById.get(dependency.pkg);
    assert.ok(
      packageMetadata,
      `${dependency.pkg} is missing from cargo metadata packages`,
    );
    assert.equal(
      packageMetadata.source,
      "registry+https://github.com/rust-lang/crates.io-index",
      `${packageMetadata.name} is not resolved from the crates.io registry`,
    );
    assert.equal(
      typeof packageMetadata.license,
      "string",
      `${packageMetadata.name} lacks reviewed Cargo license metadata`,
    );
    assert.ok(
      packageMetadata.license.trim(),
      `${packageMetadata.name} has an empty Cargo license expression`,
    );
    assert.ok(
      dependency.dep_kinds.length > 0,
      `${packageMetadata.name} lacks a Cargo dependency kind`,
    );
    for (const { kind } of dependency.dep_kinds) {
      const scope =
        kind === null
          ? "runtime"
          : kind === "build"
            ? "build"
            : kind === "dev"
              ? "development"
              : null;
      assert.ok(scope, `${packageMetadata.name} has unsupported Cargo kind`);
      rows.push({
        name: packageMetadata.name,
        version: packageMetadata.version,
        license: packageMetadata.license,
        scope,
        modified: modificationStatus(
          "cargo",
          packageMetadata.name,
          packageMetadata.version,
        ),
        source: `https://crates.io/crates/${packageMetadata.name}/${packageMetadata.version}`,
      });
    }
  }

  const uniqueRows = new Map();
  for (const row of rows) {
    const key = `${row.name}\0${row.version}\0${row.scope}`;
    const existing = uniqueRows.get(key);
    if (existing) {
      assert.deepEqual(
        row,
        existing,
        `${row.name} has conflicting Cargo metadata for one inventory scope`,
      );
    } else {
      uniqueRows.set(key, row);
    }
  }

  const scopeOrder = new Map([
    ["build", 0],
    ["runtime", 1],
    ["development", 2],
  ]);
  return [...uniqueRows.values()].sort(
    (left, right) =>
      scopeOrder.get(left.scope) - scopeOrder.get(right.scope) ||
      left.name.localeCompare(right.name) ||
      left.version.localeCompare(right.version),
  );
}

export function verifyThirdPartyInventory({
  packageJson,
  packageLock,
  cargoLock,
  cargoMetadata,
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
  assertUniqueComponents(nodeRows, "Node inventory");
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
  assertUniqueComponents(rustRows, "Rust inventory");
  assert.deepEqual(
    rustRows,
    expectedRustRows(cargoMetadata),
    "Rust inventory does not match direct manifest and lockfile metadata",
  );
}

async function main() {
  const root = process.cwd();
  const trackedDependencyFiles = execFileSync("git", ["ls-files", "-z"], {
    cwd: root,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean)
    .filter((path) =>
      /(^|\/)(?:package(?:-lock)?\.json|Cargo\.(?:toml|lock))$/u.test(path),
    )
    .sort();
  assert.deepEqual(
    trackedDependencyFiles,
    TRACKED_DEPENDENCY_FILES,
    "tracked dependency manifests/lockfiles changed; extend the THIRDPARTY verifier explicitly",
  );
  const cargoMetadata = JSON.parse(
    execFileSync(
      "cargo",
      [
        "metadata",
        "--locked",
        "--all-features",
        "--format-version",
        "1",
        "--manifest-path",
        "src-tauri/Cargo.toml",
      ],
      { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    ),
  );
  const [packageJson, packageLock, cargoLock, inventory] = await Promise.all([
    readFile(resolve(root, "package.json"), "utf8").then(JSON.parse),
    readFile(resolve(root, "package-lock.json"), "utf8").then(JSON.parse),
    readFile(resolve(root, "src-tauri/Cargo.lock"), "utf8"),
    readFile(resolve(root, INVENTORY), "utf8"),
  ]);
  verifyThirdPartyInventory({
    packageJson,
    packageLock,
    cargoLock,
    cargoMetadata,
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
