import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { verifyThirdPartyInventory } from "./verify-thirdparty.mjs";

const packageJson = {
  dependencies: { alpha: "1.2.3" },
  devDependencies: { beta: "^2.0.0" },
};
const packageLock = {
  packages: {
    "": {
      dependencies: { alpha: "1.2.3" },
      devDependencies: { beta: "^2.0.0" },
    },
    "node_modules/alpha": {
      version: "1.2.3",
      license: "MIT",
      resolved: "https://registry.npmjs.org/alpha/-/alpha-1.2.3.tgz",
    },
    "node_modules/beta": {
      version: "2.0.4",
      license: "ISC",
      resolved: "https://registry.npmjs.org/beta/-/beta-2.0.4.tgz",
    },
  },
};
const cargoLock = `[[package]]
name = "fixture"
version = "1.0.0"
dependencies = [
 "delta",
 "gamma",
]

[[package]]
name = "gamma"
version = "2.6.3"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "delta"
version = "0.12.28"
source = "registry+https://github.com/rust-lang/crates.io-index"
`;
const cargoMetadata = {
  version: 1,
  packages: [
    {
      id: "fixture 1.0.0 (path)",
      name: "fixture",
      version: "1.0.0",
      license: null,
      source: null,
    },
    {
      id: "gamma 2.6.3 (registry)",
      name: "gamma",
      version: "2.6.3",
      license: "MIT",
      source: "registry+https://github.com/rust-lang/crates.io-index",
    },
    {
      id: "delta 0.12.28 (registry)",
      name: "delta",
      version: "0.12.28",
      license: "Apache-2.0",
      source: "registry+https://github.com/rust-lang/crates.io-index",
    },
  ],
  resolve: {
    root: "fixture 1.0.0 (path)",
    nodes: [
      {
        id: "fixture 1.0.0 (path)",
        deps: [
          {
            name: "gamma",
            pkg: "gamma 2.6.3 (registry)",
            dep_kinds: [{ kind: "build", target: null }],
          },
          {
            name: "delta",
            pkg: "delta 0.12.28 (registry)",
            dep_kinds: [{ kind: null, target: null }],
          },
        ],
      },
    ],
  },
};
function cargoLockSha256(value) {
  return createHash("sha256")
    .update(value.replace(/\r\n/gu, "\n"))
    .digest("hex");
}
const inventory = `# Third-Party Components

| Component | Version | License | Scope | Modified? | Source |
| --- | --- | --- | --- | --- | --- |
| alpha | 1.2.3 | MIT | runtime | No | https://www.npmjs.com/package/alpha |
| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |

## Rust components

Normalized \`Cargo.lock\` SHA-256: \`${cargoLockSha256(cargoLock)}\`

| Component | Version | License | Scope | Modified? | Source |
| --- | --- | --- | --- | --- | --- |
| gamma | 2.6.3 | MIT | build | No | https://crates.io/crates/gamma/2.6.3 |
| delta | 0.12.28 | Apache-2.0 | runtime | No | https://crates.io/crates/delta/0.12.28 |

### Transitive Rust license review
`;

const inputs = { packageJson, packageLock, cargoLock, cargoMetadata };

const repositoryRoot = resolve(import.meta.dirname, "..");
const repositoryInputs = {
  packageJson: JSON.parse(
    readFileSync(resolve(repositoryRoot, "package.json")),
  ),
  packageLock: JSON.parse(
    readFileSync(resolve(repositoryRoot, "package-lock.json")),
  ),
  cargoLock: readFileSync(
    resolve(repositoryRoot, "src-tauri/Cargo.lock"),
    "utf8",
  ),
  cargoMetadata: JSON.parse(
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
      { cwd: repositoryRoot, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    ),
  ),
  inventory: readFileSync(resolve(repositoryRoot, "THIRDPARTY.md"), "utf8"),
};

test("accepts the current repository dependency state", () => {
  assert.doesNotThrow(() => verifyThirdPartyInventory(repositoryInputs));
});

test("accepts a complete direct-dependency inventory", () => {
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({ ...inputs, inventory }),
  );
});

test("rejects a missing Node dependency", () => {
  const changed = inventory.replace(
    "| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |\n",
    "",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects a stale Node version", () => {
  const changed = inventory.replace("| alpha | 1.2.3 |", "| alpha | 1.2.2 |");
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an incorrect Node scope", () => {
  const changed = inventory.replace(
    "| beta | 2.0.4 | ISC | development |",
    "| beta | 2.0.4 | ISC | runtime |",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an undocumented Node modification", () => {
  const changed = inventory.replace(
    "| alpha | 1.2.3 | MIT | runtime | No |",
    "| alpha | 1.2.3 | MIT | runtime | Yes |",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an incorrect Node license", () => {
  const changed = inventory.replace(
    "| alpha | 1.2.3 | MIT |",
    "| alpha | 1.2.3 | ISC |",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an incorrect Node source", () => {
  const changed = inventory.replace(
    "https://www.npmjs.com/package/alpha",
    "https://example.invalid/alpha",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects a non-registry Node lock source", () => {
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages["node_modules/alpha"].resolved =
    "git+https://example.invalid/alpha.git#0123456789abcdef";
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        packageLock: changedPackageLock,
        inventory,
      }),
    /npmjs registry/u,
  );
});

test("rejects a registry-backed npm alias", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.dependencies.alpha = "npm:beta@2.0.4";
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].dependencies.alpha = "npm:beta@2.0.4";
  changedPackageLock.packages["node_modules/alpha"].name = "beta";
  changedPackageLock.packages["node_modules/alpha"].version = "2.0.4";
  changedPackageLock.packages["node_modules/alpha"].resolved =
    "https://registry.npmjs.org/beta/-/beta-2.0.4.tgz";
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        packageJson: changedPackageJson,
        packageLock: changedPackageLock,
        inventory,
      }),
    /npm alias/u,
  );
});

test("inventories installed optional and peer Node dependencies", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.optionalDependencies = { epsilon: "^3.0.0" };
  changedPackageJson.peerDependencies = { zeta: "^4.0.0" };
  changedPackageJson.peerDependenciesMeta = { zeta: { optional: true } };
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].optionalDependencies = {
    epsilon: "^3.0.0",
  };
  changedPackageLock.packages[""].peerDependencies = { zeta: "^4.0.0" };
  changedPackageLock.packages[""].peerDependenciesMeta = {
    zeta: { optional: true },
  };
  changedPackageLock.packages["node_modules/epsilon"] = {
    version: "3.1.0",
    license: "MIT",
    optional: true,
    resolved: "https://registry.npmjs.org/epsilon/-/epsilon-3.1.0.tgz",
  };
  changedPackageLock.packages["node_modules/zeta"] = {
    version: "4.2.0",
    license: "Apache-2.0",
    peer: true,
    resolved: "https://registry.npmjs.org/zeta/-/zeta-4.2.0.tgz",
  };
  const changedInventory = inventory.replace(
    "| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |",
    `| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |
| epsilon | 3.1.0 | MIT | optional | No | https://www.npmjs.com/package/epsilon |
| zeta | 4.2.0 | Apache-2.0 | peer | No | https://www.npmjs.com/package/zeta |`,
  );
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      packageJson: changedPackageJson,
      packageLock: changedPackageLock,
      inventory: changedInventory,
    }),
  );
});

test("omits an uninstalled optional peer dependency", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.peerDependencies = { zeta: "^4.0.0" };
  changedPackageJson.peerDependenciesMeta = { zeta: { optional: true } };
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].peerDependencies = { zeta: "^4.0.0" };
  changedPackageLock.packages[""].peerDependenciesMeta = {
    zeta: { optional: true },
  };
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      packageJson: changedPackageJson,
      packageLock: changedPackageLock,
      inventory,
    }),
  );
});

test("rejects unsupported peer dependency metadata", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.peerDependencies = { zeta: "^4.0.0" };
  changedPackageJson.peerDependenciesMeta = { zeta: { optional: "yes" } };
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].peerDependencies = { zeta: "^4.0.0" };
  changedPackageLock.packages[""].peerDependenciesMeta = {
    zeta: { optional: "yes" },
  };
  assert.throws(() =>
    verifyThirdPartyInventory({
      ...inputs,
      packageJson: changedPackageJson,
      packageLock: changedPackageLock,
      inventory,
    }),
  );
});

test("uses optional scope when optionalDependencies overrides dependencies", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.dependencies.alpha = "^1.0.0";
  changedPackageJson.optionalDependencies = { alpha: "^1.2.0" };
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].dependencies.alpha = "^1.0.0";
  changedPackageLock.packages[""].optionalDependencies = {
    alpha: "^1.2.0",
  };
  changedPackageLock.packages["node_modules/alpha"].optional = true;
  const changedInventory = inventory
    .replace(
      "| alpha | 1.2.3 | MIT | runtime | No | https://www.npmjs.com/package/alpha |\n",
      "",
    )
    .replace(
      "| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |",
      `| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |
| alpha | 1.2.3 | MIT | optional | No | https://www.npmjs.com/package/alpha |`,
    );
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      packageJson: changedPackageJson,
      packageLock: changedPackageLock,
      inventory: changedInventory,
    }),
  );
});

test("inventories the same package in development and peer scopes", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.peerDependencies = { beta: "^2.0.0" };
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].peerDependencies = { beta: "^2.0.0" };
  changedPackageLock.packages["node_modules/beta"].peer = true;
  const changedInventory = inventory.replace(
    "| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |",
    `| beta | 2.0.4 | ISC | development | No | https://www.npmjs.com/package/beta |
| beta | 2.0.4 | ISC | peer | No | https://www.npmjs.com/package/beta |`,
  );
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      packageJson: changedPackageJson,
      packageLock: changedPackageLock,
      inventory: changedInventory,
    }),
  );
});

test("rejects an omitted direct optional Node dependency", () => {
  const changedPackageJson = structuredClone(packageJson);
  changedPackageJson.optionalDependencies = { epsilon: "^3.0.0" };
  const changedPackageLock = structuredClone(packageLock);
  changedPackageLock.packages[""].optionalDependencies = {
    epsilon: "^3.0.0",
  };
  changedPackageLock.packages["node_modules/epsilon"] = {
    version: "3.1.0",
    license: "MIT",
    optional: true,
    resolved: "https://registry.npmjs.org/epsilon/-/epsilon-3.1.0.tgz",
  };
  assert.throws(() =>
    verifyThirdPartyInventory({
      ...inputs,
      packageJson: changedPackageJson,
      packageLock: changedPackageLock,
      inventory,
    }),
  );
});

test("rejects duplicate components", () => {
  const row =
    "| alpha | 1.2.3 | MIT | runtime | No | https://www.npmjs.com/package/alpha |\n";
  const changed = inventory.replace(row, `${row}${row}`);
  assert.throws(
    () => verifyThirdPartyInventory({ ...inputs, inventory: changed }),
    /duplicate components/u,
  );
});

test("rejects a transitive-only Cargo.lock change", () => {
  const changedCargoLock = `${cargoLock}\n[[package]]\nname = "transitive"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n`;
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        cargoLock: changedCargoLock,
        inventory,
      }),
    /transitive Rust review/u,
  );
});

test("rejects a stale direct Rust version", () => {
  const changed = inventory.replace("| gamma | 2.6.3 |", "| gamma | 2.6.2 |");
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an incorrect direct Rust license", () => {
  const changed = inventory.replace(
    "| gamma | 2.6.3 | MIT |",
    "| gamma | 2.6.3 | AGPL-3.0-only |",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("resolves a renamed Cargo dependency to its package metadata", () => {
  const changedCargoMetadata = structuredClone(cargoMetadata);
  changedCargoMetadata.resolve.nodes[0].deps[0].name = "gamma_alias";
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      cargoMetadata: changedCargoMetadata,
      inventory,
    }),
  );
});

test("accepts one direct crate in runtime and build scopes", () => {
  const changedCargoMetadata = structuredClone(cargoMetadata);
  changedCargoMetadata.resolve.nodes[0].deps[0].dep_kinds.push({
    kind: null,
    target: null,
  });
  const runtimeRow =
    "| gamma | 2.6.3 | MIT | runtime | No | https://crates.io/crates/gamma/2.6.3 |\n";
  const changedInventory = inventory.replace(
    "| delta | 0.12.28 | Apache-2.0 | runtime | No | https://crates.io/crates/delta/0.12.28 |\n",
    `| delta | 0.12.28 | Apache-2.0 | runtime | No | https://crates.io/crates/delta/0.12.28 |\n${runtimeRow}`,
  );
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      cargoMetadata: changedCargoMetadata,
      inventory: changedInventory,
    }),
  );
});

test("consolidates one Cargo package and scope across target conditions", () => {
  const changedCargoMetadata = structuredClone(cargoMetadata);
  changedCargoMetadata.resolve.nodes[0].deps[1].dep_kinds.push({
    kind: null,
    target: "cfg(windows)",
  });
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      cargoMetadata: changedCargoMetadata,
      inventory,
    }),
  );
});

test("rejects a missing direct Rust dependency", () => {
  const row =
    "| delta | 0.12.28 | Apache-2.0 | runtime | No | https://crates.io/crates/delta/0.12.28 |\n";
  assert.throws(() =>
    verifyThirdPartyInventory({
      ...inputs,
      inventory: inventory.replace(row, ""),
    }),
  );
});

test("rejects duplicate direct Rust dependencies", () => {
  const row =
    "| gamma | 2.6.3 | MIT | build | No | https://crates.io/crates/gamma/2.6.3 |\n";
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        inventory: inventory.replace(row, `${row}${row}`),
      }),
    /duplicate components/u,
  );
});

test("rejects an incorrect direct Rust scope", () => {
  const changed = inventory.replace(
    "| delta | 0.12.28 | Apache-2.0 | runtime |",
    "| delta | 0.12.28 | Apache-2.0 | build |",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an undocumented direct Rust modification", () => {
  const changed = inventory.replace(
    "| gamma | 2.6.3 | MIT | build | No |",
    "| gamma | 2.6.3 | MIT | build | Yes |",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects an incorrect direct Rust source", () => {
  const changed = inventory.replace(
    "https://crates.io/crates/delta/0.12.28",
    "https://example.invalid/delta/0.12.28",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects direct Cargo packages outside crates.io", () => {
  const changedCargoMetadata = structuredClone(cargoMetadata);
  changedCargoMetadata.packages[1].source = "git+https://example.invalid/gamma";
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        cargoMetadata: changedCargoMetadata,
        inventory,
      }),
    /crates\.io registry/u,
  );
});

test("rejects missing Cargo license metadata", () => {
  const changedCargoMetadata = structuredClone(cargoMetadata);
  changedCargoMetadata.packages[1].license = null;
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        cargoMetadata: changedCargoMetadata,
        inventory,
      }),
    /license metadata/u,
  );
});

test("does not ignore component rows after prose in a table section", () => {
  const rogueRow =
    "| rogue | 1.0.0 | MIT | runtime | No | https://www.npmjs.com/package/rogue |";
  const changed = inventory.replace(
    "\n\n## Rust components",
    `\n\nAdditional prose.\n  ${rogueRow}\n\n## Rust components`,
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

// The release workflow asserts the produced ZIP at release time. That guard
// only runs on a tag, so these tests keep the declaration itself from being
// dropped silently on any pull request.
const RELEASE_WORKFLOW = readFileSync(
  resolve(repositoryRoot, ".github/workflows/release.yml"),
  "utf8",
);

const ARCHIVE_LEGAL_FILES = [
  "LICENSE",
  "NOTICE",
  "THIRDPARTY.md",
  "THIRD-PARTY-NOTICES.txt",
];

test("the portable archive stages every legally required file", () => {
  for (const file of ARCHIVE_LEGAL_FILES) {
    assert.ok(
      RELEASE_WORKFLOW.includes(`Copy-Item ${file} $stage`),
      `release.yml must stage ${file} into the portable archive`,
    );
  }
});

test("the release job verifies those files inside the produced ZIP", () => {
  assert.match(
    RELEASE_WORKFLOW,
    /System\.IO\.Compression\.ZipFile\]::OpenRead/u,
    "release.yml must read the produced archive back rather than trust staging",
  );
  assert.match(
    RELEASE_WORKFLOW,
    /Portable archive is missing required files/u,
    "release.yml must fail closed when a required file is absent from the ZIP",
  );
  for (const file of ARCHIVE_LEGAL_FILES) {
    assert.ok(
      RELEASE_WORKFLOW.includes(`"${file}",`),
      `${file} must appear in the archive verification list`,
    );
  }
});

test("NOTICE states where MPL-covered source can be obtained", () => {
  const notice = readFileSync(resolve(repositoryRoot, "NOTICE"), "utf8");
  assert.match(notice, /Mozilla Public License 2\.0/u);
  assert.match(notice, /THIRDPARTY\.md/u);
});

// The notices file reproduces the license text of every component embedded in
// the distributed executable. Naming a license is not preserving its notice;
// MIT, ISC, BSD and Apache-2.0 require the text and the copyright line to
// travel with the distribution.
test("CI verifies the third-party notices file on every pull request", () => {
  const ci = readFileSync(
    resolve(repositoryRoot, ".github/workflows/ci.yml"),
    "utf8",
  );
  assert.ok(
    ci.includes("npm run notices:check"),
    "ci.yml must run the notices check",
  );
  assert.ok(
    ci.includes("npm ci"),
    "the notices check reads installed artifacts, so ci.yml must install them",
  );
});

test("every vendored license fragment matches its declared digest", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const entries = Object.entries(POLICY.fragments);
  assert.ok(entries.length > 0, "policy must declare at least one fragment");
  for (const [key, fragment] of entries) {
    const text = readFileSync(resolve(repositoryRoot, fragment.path), "utf8");
    const digest = createHash("sha256").update(text, "utf8").digest("hex");
    assert.equal(
      digest,
      fragment.sha256,
      `${key} (${fragment.path}) does not match the digest declared in the policy`,
    );
  }
});

test("a supplementary notice never counts as the licence itself", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  // An Apache-2.0 NOTICE is material clause 4(d) requires alongside the
  // licence, not the licence text. If it appeared in both lists, a package
  // shipping only a NOTICE would satisfy the gate and the bundle would go out
  // incomplete.
  const portadores = new Set(POLICY.licenseFilePrefixes);
  assert.ok(portadores.size > 0, "policy must name licence-bearing prefixes");
  for (const suplementar of POLICY.supplementalFilePrefixes) {
    assert.ok(
      !portadores.has(suplementar),
      `${suplementar} must not satisfy the licence requirement on its own`,
    );
  }
});

test("every explicit licence election records what was chosen and why", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.ok(
    POLICY.licenseElectionPreference.length > 0,
    "policy must declare a preference order for unambiguous choices",
  );
  for (const [id, eleicao] of Object.entries(POLICY.licenseElections)) {
    assert.match(
      id,
      /^.+@\d+\.\d+\.\d+/u,
      `${id} must pin an exact version so the choice cannot outlive an upgrade`,
    );
    assert.ok(eleicao.expression, `${id} must record the expression it resolves`);
    assert.ok(eleicao.elected, `${id} must record the elected licence`);
    assert.ok(eleicao.rationale, `${id} must record why that licence was chosen`);
  }
});

test("every declared fallback resolves to an existing fragment", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const entries = Object.entries(POLICY.licenseFallbacks);
  assert.ok(entries.length > 0, "policy must declare at least one fallback");
  for (const [id, fallback] of entries) {
    assert.match(
      id,
      /^.+@\d+\.\d+\.\d+/u,
      `${id} must pin an exact version so the exception cannot outlive an upgrade`,
    );
    assert.ok(fallback.rationale, `${id} must record why the text is vendored`);
    assert.ok(
      fallback.sourceRepository,
      `${id} must record where the text came from`,
    );
    // A branch or tag name is not provenance: both can move, which would make
    // the recorded origin irreproducible for anyone auditing a released
    // archive later. Only a full commit id is accepted.
    assert.match(
      fallback.revision ?? "",
      /^[0-9a-f]{40}$/u,
      `${id} must pin a full commit id, not a movable ref like "${fallback.revision}"`,
    );
    for (const key of fallback.fragments) {
      assert.ok(
        POLICY.fragments[key],
        `${id} references unknown fragment ${key}`,
      );
    }
  }
});
