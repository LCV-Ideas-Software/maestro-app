import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import {
  componentesCargoDaMetadata,
  corroboradas,
  diretorioNpmExato,
  plataformaExcluida,
  resolverMetaNpm,
  selecionarRegistroOpcionalDoArtefato,
  selecionarRegistroDoArtefato,
  sha256TextoDeLicenca,
  validarEvidenciaTextual,
  validarEleicao,
  validarInspecaoManualDeLicenca,
} from "./legal/thirdparty-runtime.mjs";
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
const repositoryInputs = () => ({
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
});

test("accepts the current repository dependency state", () => {
  assert.doesNotThrow(() => verifyThirdPartyInventory(repositoryInputs()));
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
  const eleicoes = [];
  for (const [id, entrada] of Object.entries(POLICY.licenseElections)) {
    const registros = Array.isArray(entrada) ? entrada : [entrada];
    assert.ok(registros.length > 0, `${id} must declare at least one election`);
    for (const eleicao of registros) eleicoes.push([id, eleicao]);
  }
  const identidades = new Set();
  for (const [id, eleicao] of eleicoes) {
    assert.match(
      id,
      /^.+@\d+\.\d+\.\d+/u,
      `${id} must pin an exact version so the choice cannot outlive an upgrade`,
    );
    assert.ok(eleicao.expression, `${id} must record the expression it resolves`);
    assert.ok(eleicao.elected, `${id} must record the elected licence`);
    assert.ok(eleicao.rationale, `${id} must record why that licence was chosen`);
    assert.match(
      eleicao.ecosystem ?? "",
      /^(?:npm|cargo)$/u,
      `${id} must bind the election to its package ecosystem`,
    );
    assert.ok(
      eleicao.source,
      `${id} must bind the election to the exact resolved artifact origin`,
    );
    const identidade = `${id}|${eleicao.ecosystem}|${eleicao.source}`;
    assert.ok(
      !identidades.has(identidade),
      `${id} must not declare two elections for ${eleicao.ecosystem}|${eleicao.source}`,
    );
    identidades.add(identidade);
    // This is the production predicate, not a test-side reimplementation: it
    // checks both satisfiability and that every elected leaf was offered.
    assert.ok(
      validarEleicao(eleicao.expression, eleicao.elected).ok,
      `${id}: the election "${eleicao.elected}" does not satisfy "${eleicao.expression}"`,
    );
    if (eleicao.manualTextInspection) {
      assert.ok(
        eleicao.manualTextInspection.identifiedLicenses?.length,
        `${id} manual inspection must identify at least one licence`,
      );
      assert.ok(
        eleicao.manualTextInspection.rationale,
        `${id} manual inspection must record its evidence`,
      );
      assert.ok(
        eleicao.manualTextInspection.textEvidence?.length,
        `${id} manual inspection must bind the exact reproduced text set`,
      );
    }
  }
});

test("production election validation rejects OR, foreign leaves and preserves required AND and GPL plus", () => {
  const ambigua = validarEleicao(
    "MIT OR Apache-2.0",
    "MIT OR Apache-2.0",
  );
  assert.equal(ambigua.ok, false);
  assert.equal(ambigua.tipo, "eleicao-ambigua");
  const ambiguaAninhada = validarEleicao(
    "MIT AND (Apache-2.0 OR GPL-2.0-only)",
    "MIT AND (Apache-2.0 OR GPL-2.0-only)",
  );
  assert.equal(ambiguaAninhada.ok, false);
  assert.equal(ambiguaAninhada.tipo, "eleicao-ambigua");

  const forasteira = validarEleicao(
    "MIT AND Apache-2.0",
    "MIT AND Zlib",
  );
  assert.equal(forasteira.ok, false);
  assert.equal(forasteira.tipo, "forasteiras");
  assert.deepEqual(forasteira.forasteiras, ["Zlib"]);

  assert.equal(
    validarEleicao("MIT AND Apache-2.0", "MIT AND Apache-2.0").ok,
    true,
  );
  assert.equal(
    validarEleicao(
      "GPL-2.0-only WITH Classpath-exception-2.0",
      "GPL-2.0-only WITH Classpath-exception-2.0",
    ).ok,
    true,
  );

  const perdeuOuPosterior = validarEleicao("GPL-2.0+", "GPL-2.0");
  assert.equal(perdeuOuPosterior.ok, false);
  assert.equal(perdeuOuPosterior.tipo, "forasteiras");
  assert.deepEqual(perdeuOuPosterior.forasteiras, ["GPL-2.0"]);
  assert.equal(validarEleicao("GPL-2.0+", "GPL-2.0+").ok, true);

  const ramosAlternativosCombinados = validarEleicao(
    "MIT OR Apache-2.0",
    "MIT AND Apache-2.0",
  );
  assert.equal(ramosAlternativosCombinados.ok, false);
  assert.equal(ramosAlternativosCombinados.tipo, "eleicao-nao-oferecida");

  assert.equal(
    validarEleicao(
      "MIT OR (MIT AND Apache-2.0)",
      "MIT AND Apache-2.0",
    ).ok,
    true,
    "an explicit conjunctive branch remains a valid election even when another OR branch is its subset",
  );

  assert.equal(
    validarEleicao(
      "MIT AND (Apache-2.0 OR GPL-2.0-only)",
      "MIT AND Apache-2.0",
    ).ok,
    true,
  );
});

test("npm directories resolve only from the exact package-lock path", () => {
  assert.equal(
    diretorioNpmExato(repositoryRoot, "node_modules/react"),
    resolve(repositoryRoot, "node_modules/react"),
  );
  assert.equal(
    diretorioNpmExato(
      repositoryRoot,
      "node_modules/origin-b/node_modules/same-name-and-version",
    ),
    null,
    "a missing exact lock path must not fall back to another artifact with the same name and version",
  );
});

test("production election validation fails closed before combinatorial branch expansion", () => {
  const pares = [
    ["MIT", "Apache-2.0"],
    ["ISC", "BSD-2-Clause"],
    ["BSD-3-Clause", "Zlib"],
    ["MPL-2.0", "GPL-2.0-only"],
    ["GPL-3.0-only", "LGPL-2.1-only"],
    ["AGPL-3.0-only", "Unlicense"],
    ["CC0-1.0", "BSL-1.0"],
    ["Python-2.0", "Artistic-2.0"],
    ["EPL-2.0", "CDDL-1.0"],
    ["MS-PL", "0BSD"],
    ["BlueOak-1.0.0", "PostgreSQL"],
  ];
  const declarada = pares
    .map(([esquerda, direita]) => `(${esquerda} OR ${direita})`)
    .join(" AND ");
  const eleita = pares.flat().join(" AND ");

  const resultado = validarEleicao(declarada, eleita);
  assert.equal(resultado.ok, false);
  assert.equal(resultado.tipo, "eleicao-complexa");
  assert.equal(resultado.limite, 1024);
  assert.equal("ramos" in resultado, false);

  assert.equal(
    validarEleicao(
      declarada,
      pares.map(([esquerda]) => esquerda).join(" AND "),
    ).ok,
    true,
    "pruning must still accept one exact branch without expanding the incompatible alternatives",
  );
});

test("official npm platform semantics cover any, negation, libc and linked targets", () => {
  const alvo = { targetOs: "win32", targetCpu: "x64", targetLibc: null };
  assert.equal(plataformaExcluida({ os: ["any"] }, alvo), false);
  assert.equal(plataformaExcluida({ cpu: ["any"] }, alvo), false);
  assert.equal(plataformaExcluida({ os: "linux" }, alvo), true);
  assert.equal(plataformaExcluida({ os: ["!linux"] }, alvo), false);
  assert.equal(plataformaExcluida({ os: ["win32"], cpu: ["x64"] }, alvo), false);
  assert.equal(plataformaExcluida({ libc: ["glibc"] }, alvo), true);

  const packages = {
    "node_modules/local": { link: true, resolved: "packages/local" },
    "packages/local": {
      name: "local",
      version: "1.0.0",
      os: ["linux"],
    },
  };
  const resolvida = resolverMetaNpm(
    packages,
    "node_modules/local",
    packages["node_modules/local"],
  );
  assert.equal(resolvida.erro, undefined);
  assert.equal(resolvida.origemDaIdentidade, "packages/local");
  assert.equal(plataformaExcluida(resolvida.meta, alvo), true);

  const quebrada = resolverMetaNpm(
    { "node_modules/missing": { link: true, resolved: "packages/missing" } },
    "node_modules/missing",
    { link: true, resolved: "packages/missing" },
  );
  assert.match(quebrada.erro, /node_modules\/missing.*packages\/missing/u);
});

test("Cargo components use each package exact manifest for path, git and registry", () => {
  const raizId = "path+file:///repo/root#0.0.0";
  const pathId = "path+file:///repo/vendor/local#1.0.0";
  const gitId = "git+https://example.invalid/shared#1.0.0";
  const registryId = "registry+https://github.com/rust-lang/crates.io-index#shared@1.0.0";
  const pathManifest = resolve(repositoryRoot, "vendor/local/Cargo.toml");
  const gitManifest = resolve(repositoryRoot, "..", "cargo-git", "shared", "Cargo.toml");
  const registryManifest = resolve(
    repositoryRoot,
    "..",
    "cargo-registry",
    "shared-1.0.0",
    "Cargo.toml",
  );
  const metadata = {
    resolve: {
      root: raizId,
      nodes: [
        {
          id: raizId,
          deps: [pathId, gitId, registryId].map((pkg) => ({
            pkg,
            dep_kinds: [{ kind: null }],
          })),
        },
        { id: pathId, deps: [] },
        { id: gitId, deps: [] },
        { id: registryId, deps: [] },
      ],
    },
    packages: [
      {
        id: raizId,
        name: "root",
        version: "0.0.0",
        license: "AGPL-3.0-or-later",
        source: null,
        manifest_path: resolve(repositoryRoot, "src-tauri/Cargo.toml"),
      },
      {
        id: pathId,
        name: "local",
        version: "1.0.0",
        license: "MIT",
        source: null,
        manifest_path: pathManifest,
      },
      {
        id: gitId,
        name: "shared",
        version: "1.0.0",
        license: "MIT",
        source: "git+https://example.invalid/shared#0123456789abcdef",
        manifest_path: gitManifest,
      },
      {
        id: registryId,
        name: "shared",
        version: "1.0.0",
        license: "Apache-2.0",
        source: "registry+https://github.com/rust-lang/crates.io-index",
        manifest_path: registryManifest,
      },
    ],
  };

  const resultado = componentesCargoDaMetadata(metadata, {
    includedDependencyKinds: [null],
    repositoryRoot,
  });
  assert.deepEqual(resultado.erros, []);
  const local = resultado.componentes.find((componente) => componente.nome === "local");
  const compartilhados = resultado.componentes.filter(
    (componente) => componente.nome === "shared",
  );
  assert.equal(local.origemPacote, "path:vendor/local");
  assert.equal(local.diretorio, dirname(pathManifest));
  assert.equal(compartilhados.length, 2);
  assert.deepEqual(
    new Set(compartilhados.map((componente) => componente.diretorio)),
    new Set([dirname(gitManifest), dirname(registryManifest)]),
  );
  assert.equal(
    compartilhados.find((componente) => componente.origemPacote.startsWith("git+")).diretorio,
    dirname(gitManifest),
  );
});

test("Cargo path dependencies outside the repository fail closed", () => {
  const raizId = "root";
  const foraId = "outside";
  const resultado = componentesCargoDaMetadata(
    {
      resolve: {
        root: raizId,
        nodes: [
          {
            id: raizId,
            deps: [{ pkg: foraId, dep_kinds: [{ kind: null }] }],
          },
          { id: foraId, deps: [] },
        ],
      },
      packages: [
        {
          id: raizId,
          name: "root",
          version: "0.0.0",
          source: null,
          manifest_path: resolve(repositoryRoot, "src-tauri/Cargo.toml"),
        },
        {
          id: foraId,
          name: "outside",
          version: "1.0.0",
          license: "MIT",
          source: null,
          manifest_path: resolve(repositoryRoot, "..", "outside", "Cargo.toml"),
        },
      ],
    },
    { includedDependencyKinds: [null], repositoryRoot },
  );
  assert.equal(resultado.componentes.length, 0);
  assert.match(resultado.erros[0], /dependencia path fora do repositorio/u);
});

test("Unicode corroboration requires licence body, not a title or URL pointer", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.equal(
    corroboradas(
      ["Unicode-3.0"],
      [{ texto: "Unicode-3.0: https://unicode.org/license.txt" }],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
  );
  assert.equal(
    corroboradas(
      ["Unicode-3.0"],
      [
        {
          texto:
            "// Permission is hereby granted, free of charge, to any person obtaining a copy of data files",
        },
      ],
      POLICY.licenseTextMarkers,
    ).ok,
    true,
  );
});

test("MPL-2.0 corroboration requires licence body, not a title or URL pointer", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.equal(
    corroboradas(
      ["MPL-2.0"],
      [
        {
          texto:
            "Mozilla Public License 2.0: https://www.mozilla.org/MPL/2.0/",
        },
      ],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
  );
  assert.equal(
    corroboradas(
      ["MPL-2.0"],
      [
        {
          texto:
            "This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.",
        },
      ],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
  );
  assert.equal(
    corroboradas(
      ["MPL-2.0"],
      [
        {
          texto:
            "All distribution of Covered Software in Source Code Form, including any Modifications that You create or to which You contribute, must be under the terms of this License.",
        },
      ],
      POLICY.licenseTextMarkers,
    ).ok,
    true,
  );
});

test("BSL-1.0 and Python-2.0 corroboration require licence bodies, not titles or URLs", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const casos = [
    {
      licenca: "BSL-1.0",
      ponteiro:
        "Boost Software License - Version 1.0 https://www.boost.org/LICENSE_1_0.txt",
      corpo:
        "to use, reproduce, display, distribute, execute, and transmit the Software, and to prepare derivative works of the Software",
    },
    {
      licenca: "Python-2.0",
      ponteiro:
        "PYTHON SOFTWARE FOUNDATION LICENSE VERSION 2 https://docs.python.org/3/license.html",
      corpo:
        "PSF hereby grants Licensee a nonexclusive, royalty-free, world-wide license to reproduce, analyze, test, perform and/or display publicly",
    },
  ];

  for (const caso of casos) {
    assert.equal(
      corroboradas(
        [caso.licenca],
        [{ texto: caso.ponteiro }],
        POLICY.licenseTextMarkers,
      ).ok,
      false,
      `${caso.licenca} title or URL must not corroborate its licence body`,
    );
    assert.equal(
      corroboradas(
        [caso.licenca],
        [{ texto: caso.corpo }],
        POLICY.licenseTextMarkers,
      ).ok,
      true,
      `${caso.licenca} official body phrase must corroborate the licence`,
    );
  }
});

test("CC0-1.0 and CDLA-Permissive-2.0 corroboration require licence bodies, not titles or URLs", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const casos = [
    {
      licenca: "CC0-1.0",
      ponteiro:
        "Creative Commons Legal Code — CC0 1.0 Universal https://creativecommons.org/publicdomain/zero/1.0/legalcode.en",
      corpo:
        "Affirmer hereby overtly, fully, permanently, irrevocably and unconditionally waives, abandons, and surrenders all of Affirmer's Copyright and Related Rights",
    },
    {
      licenca: "CDLA-Permissive-2.0",
      ponteiro:
        "Community Data License Agreement - Permissive - Version 2.0 https://cdla.dev/permissive-2-0/",
      corpo:
        "A Data Recipient may use, modify, and share the Data made available by Data Provider(s) under this agreement",
    },
  ];

  for (const caso of casos) {
    assert.equal(
      corroboradas(
        [caso.licenca],
        [{ texto: caso.ponteiro }],
        POLICY.licenseTextMarkers,
      ).ok,
      false,
      `${caso.licenca} title or URL must not corroborate its licence body`,
    );
    assert.equal(
      corroboradas(
        [caso.licenca],
        [{ texto: caso.corpo }],
        POLICY.licenseTextMarkers,
      ).ok,
      true,
      `${caso.licenca} official body phrase must corroborate the licence`,
    );
  }
});

test("every declared supplement pins immutable provenance", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  for (const [id, entrada] of Object.entries(
    POLICY.licenseSupplements ?? {},
  )) {
    assert.match(
      id,
      /^.+@\d+\.\d+\.\d+/u,
      `${id} must pin an exact version so the exception cannot outlive an upgrade`,
    );
    const suplementos = Array.isArray(entrada) ? entrada : [entrada];
    assert.ok(suplementos.length, `${id} must declare at least one supplement`);
    const identidades = new Set();
    for (const suplemento of suplementos) {
      assert.ok(
        suplemento.rationale,
        `${id} must record why the text is vendored`,
      );
      assert.match(
        suplemento.ecosystem ?? "",
        /^(?:npm|cargo)$/u,
        `${id} must bind the supplement to an ecosystem`,
      );
      assert.match(
        suplemento.source ?? "",
        /\S/u,
        `${id} must bind the supplement to the exact resolved source`,
      );
      assert.equal(
        suplemento.source,
        suplemento.source.trim(),
        `${id} supplement source must not contain surrounding whitespace`,
      );
      const identidade = `${suplemento.ecosystem}\0${suplemento.source}`;
      assert.ok(
        !identidades.has(identidade),
        `${id} declares duplicate supplement identity ${identidade}`,
      );
      identidades.add(identidade);
      // Same requirement the fallbacks carry. The fragment digest proves the
      // local file has not changed; only a commit id proves which upstream
      // bytes it was derived from.
      assert.ok(
        suplemento.sourceRepository,
        `${id} must record where the supplementary text came from`,
      );
      assert.match(
        suplemento.revision ?? "",
        /^[0-9a-f]{40}$/u,
        `${id} must pin a full commit id, not a movable ref like "${suplemento.revision}"`,
      );
      for (const key of suplemento.fragments) {
        assert.ok(
          POLICY.fragments[key],
          `${id} references unknown fragment ${key}`,
        );
      }
    }
  }
});

// Some SPDX licence variants share every positive marker previously used by
// the gate. Such a substring cannot prove which member of the family was
// reproduced, so none of them can participate in an automatic election.
test("licences without a distinguishing marker are not elected automatically", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  for (const subconjunto of [
    "MIT-0",
    "0BSD",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Zlib",
  ]) {
    assert.ok(
      !POLICY.licenseElectionPreference.includes(subconjunto),
      `${subconjunto} has no marker that distinguishes it from the licence that contains it, so it must require an explicit election`,
    );
  }
});

test("BSD-2-Clause cannot be corroborated by text shared with BSD-3-Clause", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.equal(
    POLICY.licenseTextMarkers["BSD-2-Clause"],
    undefined,
    "BSD-2-Clause has no positive substring that distinguishes it from BSD-3-Clause",
  );
  assert.equal(
    corroboradas(
      ["BSD-2-Clause"],
      [
        {
          texto:
            "Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.",
        },
      ],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
  );

  const esperadas = new Map([
    [
      "dingbat-to-unicode@1.0.1",
      "https://registry.npmjs.org/dingbat-to-unicode/-/dingbat-to-unicode-1.0.1.tgz",
    ],
    [
      "entities@4.5.0",
      "https://registry.npmjs.org/entities/-/entities-4.5.0.tgz",
    ],
    ["lop@0.4.2", "https://registry.npmjs.org/lop/-/lop-0.4.2.tgz"],
    [
      "mammoth@1.12.1",
      "https://registry.npmjs.org/mammoth/-/mammoth-1.12.1.tgz",
    ],
    ["option@0.2.4", "https://registry.npmjs.org/option/-/option-0.2.4.tgz"],
  ]);
  for (const [id, source] of esperadas) {
    const inspecao = POLICY.unverifiableLicenseDeclarations[id];
    assert.ok(inspecao, `${id} must have an artifact-specific inspection`);
    assert.equal(inspecao.ecosystem, "npm");
    assert.equal(inspecao.source, source);
    assert.equal(inspecao.declared, "BSD-2-Clause");
    assert.equal(inspecao.identifiedLicense, "BSD-2-Clause");
    assert.ok(inspecao.rationale, `${id} must record inspection evidence`);
  }
});

test("BSD-3-Clause cannot be corroborated by BSD-4-Clause text", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.equal(POLICY.licenseTextMarkers["BSD-3-Clause"], undefined);
  const textoBsd4 = [
    "All advertising materials mentioning features or use of this software must display the following acknowledgement.",
    "Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.",
  ].join("\n");
  assert.equal(
    corroboradas(
      ["BSD-3-Clause"],
      [{ texto: textoBsd4 }],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
  );

  const esperadas = [
    [
      "highlight.js@11.11.1",
      "npm",
      "https://registry.npmjs.org/highlight.js/-/highlight.js-11.11.1.tgz",
    ],
    [
      "sprintf-js@1.0.3",
      "npm",
      "https://registry.npmjs.org/sprintf-js/-/sprintf-js-1.0.3.tgz",
    ],
    [
      "subtle@2.6.1",
      "cargo",
      "registry+https://github.com/rust-lang/crates.io-index",
    ],
  ];
  for (const [id, ecosystem, source] of esperadas) {
    const inspecao = POLICY.unverifiableLicenseDeclarations[id];
    assert.equal(inspecao.ecosystem, ecosystem);
    assert.equal(inspecao.source, source);
    assert.equal(inspecao.declared, "BSD-3-Clause");
    assert.equal(inspecao.identifiedLicense, "BSD-3-Clause");
    assert.ok(inspecao.rationale);
  }

  const inspecao = selecionarRegistroDoArtefato(
    POLICY.unverifiableLicenseDeclarations["highlight.js@11.11.1"],
    {
      ecossistema: "npm",
      origemPacote:
        "https://registry.npmjs.org/highlight.js/-/highlight.js-11.11.1.tgz",
    },
  ).registro;
  const textoReal = readFileSync(
    resolve(repositoryRoot, "node_modules/highlight.js/LICENSE"),
    "utf8",
  );
  assert.equal(
    validarEvidenciaTextual(inspecao, [
      { arquivo: "LICENSE", texto: textoReal },
    ]).ok,
    true,
  );
  const mutante = validarEvidenciaTextual(inspecao, [
    { arquivo: "LICENSE", texto: textoBsd4 },
  ]);
  assert.equal(mutante.ok, false);
  assert.equal(mutante.tipo, "texto-divergente");
});

test("Zlib cannot be corroborated by zlib-acknowledgement text", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.equal(POLICY.licenseTextMarkers.Zlib, undefined);
  const textoComReconhecimentoObrigatorio = [
    "This software is provided 'as-is', without any express or implied warranty.",
    "If you use this software in a product, an acknowledgment (see the following) in the product documentation is required.",
    "Altered source versions must be plainly marked as such.",
  ].join("\n");
  assert.equal(
    corroboradas(
      ["Zlib"],
      [{ texto: textoComReconhecimentoObrigatorio }],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
  );

  const origemPako =
    "https://registry.npmjs.org/pako/-/pako-1.0.11.tgz";
  const pako = selecionarRegistroDoArtefato(
    POLICY.licenseElections["pako@1.0.11"],
    { ecossistema: "npm", origemPacote: origemPako },
  ).registro;
  assert.ok(pako, "pako npm registry election must be source-qualified");
  assert.equal(pako.ecosystem, "npm");
  assert.equal(pako.source, origemPako);
  assert.deepEqual(pako.manualTextInspection?.identifiedLicenses, ["Zlib"]);
  assert.ok(pako.manualTextInspection?.rationale);

  const textosReais = [
    {
      arquivo: "LICENSE",
      texto: readFileSync(
        resolve(repositoryRoot, "node_modules/pako/LICENSE"),
        "utf8",
      ),
    },
    {
      arquivo: "scripts/legal/pako-zlib.txt",
      texto: readFileSync(
        resolve(repositoryRoot, "scripts/legal/pako-zlib.txt"),
        "utf8",
      ),
    },
  ];
  assert.equal(
    validarEvidenciaTextual(pako.manualTextInspection, textosReais).ok,
    true,
  );
  const mutante = validarEvidenciaTextual(pako.manualTextInspection, [
    textosReais[0],
    {
      arquivo: "scripts/legal/pako-zlib.txt",
      texto: textoComReconhecimentoObrigatorio,
    },
  ]);
  assert.equal(mutante.ok, false);
  assert.equal(mutante.tipo, "texto-divergente");
});

test("foldhash Zlib inspection is bound to the exact crates.io artifact", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const textoZlib = [
    "This software is provided 'as-is', without any express or implied warranty.",
    "If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.",
    "Altered source versions must be plainly marked as such.",
  ].join("\n");
  assert.equal(
    corroboradas(
      ["Zlib"],
      [{ texto: textoZlib }],
      POLICY.licenseTextMarkers,
    ).ok,
    false,
    "ordinary Zlib text has no safe positive marker and needs exact inspection",
  );

  const entrada = POLICY.unverifiableLicenseDeclarations["foldhash@0.2.0"];
  const origemCratesIo =
    "registry+https://github.com/rust-lang/crates.io-index";
  const inspecao = selecionarRegistroDoArtefato(entrada, {
    ecossistema: "cargo",
    origemPacote: origemCratesIo,
  });
  assert.equal(inspecao.ok, true);
  assert.equal(inspecao.registro.declared, "Zlib");
  assert.equal(inspecao.registro.identifiedLicense, "Zlib");
  assert.ok(inspecao.registro.rationale);
  assert.equal(
    validarEvidenciaTextual(inspecao.registro, [
      { arquivo: "LICENSE", texto: textoZlib },
    ]).tipo,
    "texto-divergente",
  );

  const substituido = selecionarRegistroDoArtefato(entrada, {
    ecossistema: "cargo",
    origemPacote: "path:vendor/foldhash",
  });
  assert.equal(substituido.ok, false);
  assert.equal(substituido.tipo, "origem-divergente");
});

test("every manual licence inspection binds an exact artifact and records its finding", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  for (const [id, entrada] of Object.entries(
    POLICY.unverifiableLicenseDeclarations,
  )) {
    const inspecoes = Array.isArray(entrada) ? entrada : [entrada];
    assert.ok(
      inspecoes.length > 0,
      `${id} must declare at least one inspection`,
    );
    const identidades = new Set();
    for (const inspecao of inspecoes) {
      assert.match(inspecao.ecosystem ?? "", /^(?:npm|cargo)$/u);
      assert.ok(inspecao.source, `${id} must bind the inspected source`);
      assert.ok(
        inspecao.declared === null ||
          (typeof inspecao.declared === "string" &&
            inspecao.declared.trim().length > 0),
        `${id} must record the publisher declaration or explicit null when metadata is absent`,
      );
      assert.ok(
        inspecao.identifiedLicense,
        `${id} must record the licence identified in the artifact`,
      );
      assert.ok(inspecao.rationale, `${id} must record inspection evidence`);
      assert.ok(
        Array.isArray(inspecao.textEvidence) && inspecao.textEvidence.length,
        `${id} must bind the complete inspected text set`,
      );
      const arquivos = new Set();
      for (const evidencia of inspecao.textEvidence) {
        assert.ok(evidencia.file, `${id} text evidence must name its file`);
        assert.match(
          evidencia.sha256 ?? "",
          /^[0-9a-f]{64}$/u,
          `${id} text evidence must pin a lowercase SHA-256`,
        );
        assert.ok(
          !arquivos.has(evidencia.file),
          `${id} text evidence must not repeat ${evidencia.file}`,
        );
        arquivos.add(evidencia.file);
      }
      const identidade = `${inspecao.ecosystem}|${inspecao.source}`;
      assert.ok(
        !identidades.has(identidade),
        `${id} must not declare two inspections for ${identidade}`,
      );
      identidades.add(identidade);
    }
  }
});

test("manual text evidence rejects changed, added, removed and malformed material", () => {
  const texto = "licence body\nsecond line";
  const inspecao = {
    textEvidence: [
      { file: "LICENSE", sha256: sha256TextoDeLicenca(texto) },
    ],
  };
  assert.deepEqual(
    validarEvidenciaTextual(inspecao, [{ arquivo: "LICENSE", texto }]),
    { ok: true },
  );

  for (const [tipo, textos] of [
    ["texto-divergente", [{ arquivo: "LICENSE", texto: `${texto} changed` }]],
    [
      "conjunto-divergente",
      [
        { arquivo: "LICENSE", texto },
        { arquivo: "NOTICE", texto: "new material" },
      ],
    ],
    ["conjunto-divergente", []],
  ]) {
    const resultado = validarEvidenciaTextual(inspecao, textos);
    assert.equal(resultado.ok, false);
    assert.equal(resultado.tipo, tipo);
  }

  const incompleta = validarEvidenciaTextual(
    { textEvidence: [{ file: "LICENSE", sha256: "not-a-sha256" }] },
    [{ arquivo: "LICENSE", texto }],
  );
  assert.equal(incompleta.ok, false);
  assert.equal(incompleta.tipo, "politica-incompleta");

  const duplicada = validarEvidenciaTextual(
    {
      textEvidence: [
        { file: "LICENSE", sha256: sha256TextoDeLicenca(texto) },
        { file: "LICENSE", sha256: sha256TextoDeLicenca(texto) },
      ],
    },
    [{ arquivo: "LICENSE", texto }],
  );
  assert.equal(duplicada.ok, false);
  assert.equal(duplicada.tipo, "arquivo-duplicado");
});

test("absent licence metadata fails closed unless an exact manual inspection covers it", () => {
  const texto = "Permission is hereby granted for this exact inspected fixture.";
  const source =
    "https://registry.npmjs.org/no-metadata/-/no-metadata-1.0.0.tgz";
  const inspecao = {
    ecosystem: "npm",
    source,
    declared: null,
    identifiedLicense: "MIT",
    rationale: "The publisher omitted metadata; the bundled text was inspected.",
    textEvidence: [
      { file: "LICENSE", sha256: sha256TextoDeLicenca(texto) },
    ],
  };
  const componente = {
    ecossistema: "npm",
    origemPacote: source,
    licencaDeclarada: null,
  };

  const selecionada = selecionarRegistroDoArtefato(inspecao, componente);
  assert.equal(selecionada.ok, true);
  assert.equal(
    validarInspecaoManualDeLicenca(
      selecionada.registro,
      componente.licencaDeclarada,
      [{ arquivo: "LICENSE", texto }],
    ).ok,
    true,
  );

  const semInspecao = validarInspecaoManualDeLicenca(
    null,
    componente.licencaDeclarada,
    [{ arquivo: "LICENSE", texto }],
  );
  assert.equal(semInspecao.ok, false);
  assert.equal(semInspecao.tipo, "inspecao-incompleta");

  const origemTrocada = selecionarRegistroDoArtefato(inspecao, {
    ...componente,
    origemPacote: "git+https://example.invalid/fork.git#deadbeef",
  });
  assert.equal(origemTrocada.ok, false);
  assert.equal(origemTrocada.tipo, "origem-divergente");

  const textoMutado = validarInspecaoManualDeLicenca(
    inspecao,
    componente.licencaDeclarada,
    [{ arquivo: "LICENSE", texto: `${texto} mutated` }],
  );
  assert.equal(textoMutado.ok, false);
  assert.equal(textoMutado.tipo, "texto-divergente");
});

test("every declared fallback resolves to an existing fragment", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const entries = Object.entries(POLICY.licenseFallbacks);
  assert.ok(entries.length > 0, "policy must declare at least one fallback");
  for (const [id, entrada] of entries) {
    assert.match(
      id,
      /^.+@\d+\.\d+\.\d+/u,
      `${id} must pin an exact version so the exception cannot outlive an upgrade`,
    );
    const fallbacks = Array.isArray(entrada) ? entrada : [entrada];
    assert.ok(fallbacks.length, `${id} must declare at least one fallback`);
    const identidades = new Set();
    for (const fallback of fallbacks) {
      assert.ok(
        fallback.rationale,
        `${id} must record why the text is vendored`,
      );
      assert.match(
        fallback.ecosystem ?? "",
        /^(?:npm|cargo)$/u,
        `${id} must bind the fallback to an ecosystem`,
      );
      assert.match(
        fallback.source ?? "",
        /\S/u,
        `${id} must bind the fallback to the exact resolved source`,
      );
      assert.equal(
        fallback.source,
        fallback.source.trim(),
        `${id} fallback source must not contain surrounding whitespace`,
      );
      const identidade = `${fallback.ecosystem}\0${fallback.source}`;
      assert.ok(
        !identidades.has(identidade),
        `${id} declares duplicate fallback identity ${identidade}`,
      );
      identidades.add(identidade);
      const textSourceRepository =
        fallback.textSourceRepository ?? fallback.sourceRepository;
      const textRevision = fallback.textRevision ?? fallback.revision;
      assert.ok(
        textSourceRepository,
        `${id} must record where the text came from`,
      );
      // A branch or tag name is not provenance: both can move, which would
      // make the recorded origin irreproducible for anyone auditing a released
      // archive later. Only a full commit id is accepted.
      assert.match(
        textRevision ?? "",
        /^[0-9a-f]{40}$/u,
        `${id} must pin a full commit id, not a movable ref like "${textRevision}"`,
      );
      if (fallback.copyrightSourceRepository) {
        assert.match(
          fallback.copyrightRevision ?? "",
          /^[0-9a-f]{40}$/u,
          `${id} must pin the separate copyright-source revision`,
        );
        assert.ok(
          fallback.copyrightSourcePath,
          `${id} must record the copyright-source path`,
        );
      }
      for (const key of fallback.fragments) {
        assert.ok(
          POLICY.fragments[key],
          `${id} references unknown fragment ${key}`,
        );
      }
    }
  }
});

test("siphasher fallback separates pinned MIT text and copyright provenance", async () => {
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  const fallback = POLICY.licenseFallbacks["siphasher@1.0.2"];
  assert.equal(
    fallback.textSourceRepository,
    "https://github.com/spdx/license-list-data",
  );
  assert.equal(
    fallback.textRevision,
    "c4a7237ec8f4654e867546f9f409749300f1bf4c",
  );
  assert.equal(fallback.textSourcePath, "text/MIT.txt");
  assert.equal(
    fallback.copyrightSourceRepository,
    "https://github.com/jedisct1/rust-siphash",
  );
  assert.equal(
    fallback.copyrightRevision,
    "db8172048a1c9bdef0dcec782d965c236161af13",
  );
  assert.equal(fallback.copyrightSourcePath, "COPYING");
  assert.notEqual(fallback.textRevision, fallback.copyrightRevision);

  const notice = readFileSync(
    resolve(repositoryRoot, "THIRD-PARTY-NOTICES.txt"),
    "utf8",
  );
  const inicio = notice.indexOf("siphasher 1.0.2  (cargo)");
  assert.notEqual(inicio, -1, "generated notices must contain siphasher");
  const bloco = notice.slice(inicio, inicio + 4_000);
  assert.match(
    bloco,
    /Origem do texto: https:\/\/github\.com\/spdx\/license-list-data @ [0-9a-f]{40} \(text\/MIT\.txt\)/u,
  );
  assert.match(
    bloco,
    /Origem do aviso de copyright: https:\/\/github\.com\/jedisct1\/rust-siphash @ [0-9a-f]{40} \(COPYING\)/u,
  );
});

test("explicit elections bind ecosystem and exact resolved artifact origin", async () => {
  const runtime = await import("./legal/thirdparty-runtime.mjs");
  const { POLICY } = await import("./legal/thirdparty-policy.mjs");
  assert.equal(typeof runtime.validarVinculoDoArtefato, "function");

  const entrada = POLICY.licenseElections["serial2@0.2.37"];
  const eleicao = runtime.selecionarRegistroDoArtefato(entrada, {
    ecossistema: "cargo",
    origemPacote: "registry+https://github.com/rust-lang/crates.io-index",
  }).registro;
  assert.ok(eleicao, "serial2 crates.io election must be source-qualified");
  assert.equal(eleicao.ecosystem, "cargo");
  assert.equal(
    eleicao.source,
    "registry+https://github.com/rust-lang/crates.io-index",
  );
  assert.deepEqual(
    runtime.validarVinculoDoArtefato(eleicao, {
      ecossistema: "cargo",
      origemPacote: "registry+https://github.com/rust-lang/crates.io-index",
    }),
    { ok: true },
  );

  const incompleta = runtime.validarVinculoDoArtefato({}, {
    ecossistema: "cargo",
    origemPacote: eleicao.source,
  });
  assert.equal(incompleta.ok, false);
  assert.equal(incompleta.tipo, "politica-incompleta");

  for (const origemPacote of [
    "git+https://example.invalid/serial2#0123456789abcdef",
    "path:vendor/serial2",
    `${eleicao.source}/`,
  ]) {
    const resultado = runtime.validarVinculoDoArtefato(eleicao, {
      ecossistema: "cargo",
      origemPacote,
    });
    assert.equal(resultado.ok, false);
    assert.equal(resultado.tipo, "origem-divergente");
  }

  const ecossistema = runtime.validarVinculoDoArtefato(eleicao, {
    ecossistema: "npm",
    origemPacote: eleicao.source,
  });
  assert.equal(ecossistema.ok, false);
  assert.equal(ecossistema.tipo, "ecossistema-divergente");
});

test("source-qualified policy buckets select full artifact identity and reject duplicates", async () => {
  const runtime = await import("./legal/thirdparty-runtime.mjs");
  assert.equal(typeof runtime.selecionarRegistroDoArtefato, "function");

  const registry = {
    ecosystem: "cargo",
    source: "registry+https://github.com/rust-lang/crates.io-index",
    expression: "MIT OR Apache-2.0",
    elected: "MIT",
  };
  const git = {
    ecosystem: "cargo",
    source: "git+https://example.invalid/shared#0123456789abcdef",
    expression: "MIT OR Apache-2.0",
    elected: "Apache-2.0",
  };
  const npm = {
    ecosystem: "npm",
    source: "https://registry.npmjs.org/shared/-/shared-1.0.0.tgz",
    expression: "MIT OR Apache-2.0",
    elected: "MIT",
  };

  assert.deepEqual(
    runtime.selecionarRegistroDoArtefato([registry, git, npm], {
      ecossistema: "cargo",
      origemPacote: git.source,
    }),
    { ok: true, registro: git },
  );
  assert.deepEqual(
    runtime.selecionarRegistroDoArtefato(registry, {
      ecossistema: "cargo",
      origemPacote: registry.source,
    }),
    { ok: true, registro: registry },
    "legacy single-object policy entries must remain supported",
  );

  const ausente = runtime.selecionarRegistroDoArtefato([registry, git], {
    ecossistema: "cargo",
    origemPacote: "path:vendor/shared",
  });
  assert.equal(ausente.ok, false);
  assert.equal(ausente.tipo, "origem-divergente");

  const duplicada = runtime.selecionarRegistroDoArtefato(
    [registry, { ...registry, elected: "Apache-2.0" }],
    { ecossistema: "cargo", origemPacote: registry.source },
  );
  assert.equal(duplicada.ok, false);
  assert.equal(duplicada.tipo, "politica-duplicada");
});

test("optional vendored text selects full artifact identity and falls through when no source matches", () => {
  const registry = {
    ecosystem: "cargo",
    source: "registry+https://github.com/rust-lang/crates.io-index",
    fragments: ["mit"],
  };
  const git = {
    ecosystem: "cargo",
    source: "git+https://example.invalid/shared#0123456789abcdef",
    fragments: ["apache"],
  };

  assert.deepEqual(
    selecionarRegistroOpcionalDoArtefato([registry, git], {
      ecossistema: "cargo",
      origemPacote: git.source,
    }),
    { ok: true, registro: git },
    "the exact source-qualified vendored text must be selected",
  );
  assert.deepEqual(
    selecionarRegistroOpcionalDoArtefato(registry, {
      ecossistema: "cargo",
      origemPacote: "path:vendor/shared",
    }),
    { ok: true, registro: null },
    "another origin with the same name/version must fall through to its own artifact text",
  );

  const incompleta = selecionarRegistroOpcionalDoArtefato(
    { ecosystem: "cargo", fragments: ["mit"] },
    {
      ecossistema: "cargo",
      origemPacote: registry.source,
    },
  );
  assert.equal(incompleta.ok, false);
  assert.equal(incompleta.tipo, "politica-incompleta");

  for (const entradaMalformada of [
    { ecosystem: "cargo", source: "", fragments: ["mit"] },
    { ecosystem: "cargo", source: "   ", fragments: ["mit"] },
    { ecosystem: "", source: registry.source, fragments: ["mit"] },
    { ecosystem: "npn", source: registry.source, fragments: ["mit"] },
  ]) {
    const malformada = selecionarRegistroOpcionalDoArtefato(
      entradaMalformada,
      {
        ecossistema: "cargo",
        origemPacote: registry.source,
      },
    );
    assert.equal(malformada.ok, false);
    assert.equal(malformada.tipo, "politica-incompleta");
  }

  const misturaMalformada = selecionarRegistroOpcionalDoArtefato(
    [registry, { ecosystem: " ", source: " ", fragments: ["apache"] }],
    {
      ecossistema: "cargo",
      origemPacote: registry.source,
    },
  );
  assert.equal(misturaMalformada.ok, false);
  assert.equal(misturaMalformada.tipo, "politica-incompleta");

  const duplicada = selecionarRegistroOpcionalDoArtefato(
    [registry, { ...registry, fragments: ["apache"] }],
    {
      ecossistema: "cargo",
      origemPacote: registry.source,
    },
  );
  assert.equal(duplicada.ok, false);
  assert.equal(duplicada.tipo, "politica-duplicada");
});
