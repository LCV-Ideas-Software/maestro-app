import assert from "node:assert/strict";
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
    "node_modules/alpha": { version: "1.2.3", license: "MIT" },
    "node_modules/beta": { version: "2.0.4", license: "ISC" },
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
const inventory = `# Third-Party Components

| Component | Version | License | Scope | Source |
| --- | --- | --- | --- | --- |
| alpha | 1.2.3 | MIT | runtime | https://www.npmjs.com/package/alpha |
| beta | 2.0.4 | ISC | development | https://www.npmjs.com/package/beta |

## Rust components

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
  assert.throws(
    () =>
      verifyThirdPartyInventory({
        ...inputs,
        cargoLock: changedCargoLock,
        inventory,
      }),
    /crates.io registry/u,
  );
});
