import assert from "node:assert/strict";
import { createHash } from "node:crypto";
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
const cargoToml = `[build-dependencies]
gamma = "2.6.2"

[dependencies]
delta = { version = "0.12", features = [] }
`;
const cargoLock = `[[package]]
name = "gamma"
version = "2.6.3"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "delta"
version = "0.12.28"
source = "registry+https://github.com/rust-lang/crates.io-index"
`;
function cargoLockSha256(value) {
  return createHash("sha256")
    .update(value.replace(/\r\n/gu, "\n"))
    .digest("hex");
}
const inventory = `# Third-Party Components

| Component | Version | License | Scope | Source |
| --- | --- | --- | --- | --- |
| alpha | 1.2.3 | MIT | runtime | https://www.npmjs.com/package/alpha |
| beta | 2.0.4 | ISC | development | https://www.npmjs.com/package/beta |

## Rust components

Normalized \`Cargo.lock\` SHA-256: \`${cargoLockSha256(cargoLock)}\`

| Component | Version | License | Scope | Source |
| --- | --- | --- | --- | --- |
| gamma | 2.6.3 | MIT | build | https://crates.io/crates/gamma/2.6.3 |
| delta | 0.12.28 | Apache-2.0 | runtime | https://crates.io/crates/delta/0.12.28 |

### Transitive Rust license review
`;

const inputs = { packageJson, packageLock, cargoToml, cargoLock };

test("accepts a complete direct-dependency inventory", () => {
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({ ...inputs, inventory }),
  );
});

test("rejects a missing Node dependency", () => {
  const changed = inventory.replace(
    "| beta | 2.0.4 | ISC | development | https://www.npmjs.com/package/beta |\n",
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

test("rejects unsupported direct Node dependency classes", () => {
  const changedPackageJson = {
    ...packageJson,
    optionalDependencies: { optional: "1.0.0" },
  };
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        packageJson: changedPackageJson,
        inventory,
      }),
    /optionalDependencies/u,
  );
});

test("rejects duplicate components", () => {
  const row =
    "| alpha | 1.2.3 | MIT | runtime | https://www.npmjs.com/package/alpha |\n";
  const changed = inventory.replace(row, `${row}${row}`);
  assert.throws(
    () => verifyThirdPartyInventory({ ...inputs, inventory: changed }),
    /duplicate components/u,
  );
});

test("rejects a stale direct Rust version", () => {
  const changed = inventory.replace("| gamma | 2.6.3 |", "| gamma | 2.6.2 |");
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("accepts Cargo caret updates across a nonzero minor version", () => {
  const changedCargoToml = cargoToml.replace(
    'gamma = "2.6.2"',
    'gamma = "2.6"',
  );
  const changedCargoLock = cargoLock.replace(
    'version = "2.6.3"',
    'version = "2.7.0"',
  );
  const changedInventory = inventory
    .replace(cargoLockSha256(cargoLock), cargoLockSha256(changedCargoLock))
    .replace("| gamma | 2.6.3 |", "| gamma | 2.7.0 |")
    .replace("/gamma/2.6.3 |", "/gamma/2.7.0 |");
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      cargoToml: changedCargoToml,
      cargoLock: changedCargoLock,
      inventory: changedInventory,
    }),
  );
});

test("accepts Cargo 0.0 caret updates when only two components are declared", () => {
  const changedCargoToml = cargoToml.replace(
    'gamma = "2.6.2"',
    'gamma = "0.0"',
  );
  const changedCargoLock = cargoLock.replace(
    'version = "2.6.3"',
    'version = "0.0.5"',
  );
  const changedInventory = inventory
    .replace(cargoLockSha256(cargoLock), cargoLockSha256(changedCargoLock))
    .replace("| gamma | 2.6.3 |", "| gamma | 0.0.5 |")
    .replace("/gamma/2.6.3 |", "/gamma/0.0.5 |");
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      cargoToml: changedCargoToml,
      cargoLock: changedCargoLock,
      inventory: changedInventory,
    }),
  );
});

test("rejects prerelease locks for stable Cargo requirements", () => {
  for (const [requirement, stableVersion, prereleaseVersion] of [
    ["1.0.0", "2.6.3", "1.0.0-alpha.1"],
    ["0.0.3", "2.6.3", "0.0.3-alpha.1"],
  ]) {
    const changedCargoToml = cargoToml.replace(
      'gamma = "2.6.2"',
      `gamma = "${requirement}"`,
    );
    const changedCargoLock = cargoLock.replace(
      `version = "${stableVersion}"`,
      `version = "${prereleaseVersion}"`,
    );
    const changedInventory = inventory
      .replace(cargoLockSha256(cargoLock), cargoLockSha256(changedCargoLock))
      .replace("| gamma | 2.6.3 |", `| gamma | ${prereleaseVersion} |`)
      .replace("/gamma/2.6.3 |", `/gamma/${prereleaseVersion} |`);
    assert.throws(
      () =>
        verifyThirdPartyInventory({
          ...inputs,
          cargoToml: changedCargoToml,
          cargoLock: changedCargoLock,
          inventory: changedInventory,
        }),
      /must resolve to exactly one matching package/u,
    );
  }
});

test("ignores a transitive prerelease when a stable Cargo candidate matches", () => {
  const prereleaseBlock = `[[package]]
name = "gamma"
version = "2.6.3-alpha.1"
source = "registry+https://github.com/rust-lang/crates.io-index"

`;
  const changedCargoLock = `${prereleaseBlock}${cargoLock}`;
  const changedInventory = inventory.replace(
    cargoLockSha256(cargoLock),
    cargoLockSha256(changedCargoLock),
  );
  assert.doesNotThrow(() =>
    verifyThirdPartyInventory({
      ...inputs,
      cargoLock: changedCargoLock,
      inventory: changedInventory,
    }),
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

test("rejects a missing direct Rust dependency", () => {
  const row =
    "| delta | 0.12.28 | Apache-2.0 | runtime | https://crates.io/crates/delta/0.12.28 |\n";
  assert.throws(() =>
    verifyThirdPartyInventory({
      ...inputs,
      inventory: inventory.replace(row, ""),
    }),
  );
});

test("rejects duplicate direct Rust dependencies", () => {
  const row =
    "| gamma | 2.6.3 | MIT | build | https://crates.io/crates/gamma/2.6.3 |\n";
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

test("rejects an incorrect direct Rust source", () => {
  const changed = inventory.replace(
    "https://crates.io/crates/delta/0.12.28",
    "https://example.invalid/delta/0.12.28",
  );
  assert.throws(() =>
    verifyThirdPartyInventory({ ...inputs, inventory: changed }),
  );
});

test("rejects unmodeled Cargo dependency sections", () => {
  const changedCargoToml = `${cargoToml}\n[dev-dependencies]\nepsilon = "1"\n`;
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        cargoToml: changedCargoToml,
        inventory,
      }),
    /unsupported Cargo dependency section/u,
  );
});

test("rejects non-crates.io registry sources", () => {
  const changedCargoLock = cargoLock.replace(
    "registry+https://github.com/rust-lang/crates.io-index",
    "registry+https://example.invalid/index",
  );
  const changedInventory = inventory.replace(
    cargoLockSha256(cargoLock),
    cargoLockSha256(changedCargoLock),
  );
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        cargoLock: changedCargoLock,
        inventory: changedInventory,
      }),
    /crates.io registry/u,
  );
});
