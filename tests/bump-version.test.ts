import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

const SCRIPT_SRC = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../scripts/bump-version.mjs",
);

// One fixture per file the real release process touches, parameterized by
// version so tests can set up a consistent starting point and then drift
// one file to exercise the safety check. Each fixture also carries an
// unrelated "version" field that happens to share the same value, to prove
// the script's anchors don't spill onto it.
const FIXTURES: Record<string, (version: string) => string> = {
  "package.json": (version) =>
    JSON.stringify({ name: "vault-buddy", private: true, version }, null, 2) + "\n",
  "package-lock.json": (version) =>
    JSON.stringify(
      {
        name: "vault-buddy",
        version,
        lockfileVersion: 3,
        requires: true,
        packages: {
          "": {
            name: "vault-buddy",
            version,
            dependencies: { "some-dep": `^${version}` },
          },
          "node_modules/some-dep": {
            version,
            resolved: `https://example.com/some-dep-${version}.tgz`,
          },
        },
      },
      null,
      2,
    ) + "\n",
  "src-tauri/tauri.conf.json": (version) =>
    JSON.stringify(
      {
        $schema: "https://schema.tauri.app/config/2",
        productName: "Vault Buddy",
        version,
        identifier: "com.vaultbuddy.desktop",
      },
      null,
      2,
    ) + "\n",
  "src-tauri/Cargo.toml": (version) =>
    [
      "[package]",
      'name = "vault-buddy"',
      `version = "${version}"`,
      'description = "test fixture"',
      'edition = "2021"',
      "",
      "[dependencies]",
      `some-crate = { version = "${version}" }`,
      "",
    ].join("\n"),
  "src-tauri/Cargo.lock": (version) =>
    [
      "[[package]]",
      'name = "some-crate"',
      `version = "${version}"`,
      "dependencies = [",
      "]",
      "",
      "[[package]]",
      'name = "vault-buddy"',
      `version = "${version}"`,
      "dependencies = [",
      ' "some-crate",',
      "]",
      "",
    ].join("\n"),
};

function writeFixtures(dir: string, version: string): void {
  for (const [relPath, render] of Object.entries(FIXTURES)) {
    const filePath = path.join(dir, relPath);
    mkdirSync(path.dirname(filePath), { recursive: true });
    writeFileSync(filePath, render(version));
  }
  mkdirSync(path.join(dir, "scripts"), { recursive: true });
  copyFileSync(SCRIPT_SRC, path.join(dir, "scripts", "bump-version.mjs"));
}

function readFixture(dir: string, relPath: string): string {
  return readFileSync(path.join(dir, relPath), "utf8");
}

function runScript(dir: string, args: string[]) {
  return spawnSync(process.execPath, [path.join(dir, "scripts", "bump-version.mjs"), ...args], {
    cwd: dir,
    encoding: "utf8" as const,
  });
}

describe("bump-version.mjs", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "bump-version-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("errors with a usage message when called with the wrong number of arguments", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, []);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("Usage: bump-version.mjs");
  });

  it("--check reports OK and exits 0 when all files agree", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, ["--check"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("OK: all files agree on version 1.2.3");
  });

  it("--check exits non-zero and names the offending file when versions have drifted", () => {
    writeFixtures(dir, "1.2.3");
    writeFileSync(
      path.join(dir, "src-tauri/tauri.conf.json"),
      FIXTURES["src-tauri/tauri.conf.json"]("1.2.4"),
    );
    const result = runScript(dir, ["--check"]);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("src-tauri/tauri.conf.json");
  });

  it("bumps all 5 files to an explicit version, leaving unrelated version fields untouched", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, ["1.3.0"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("1.2.3 -> 1.3.0");

    expect(JSON.parse(readFixture(dir, "package.json")).version).toBe("1.3.0");

    const lock = JSON.parse(readFixture(dir, "package-lock.json"));
    expect(lock.version).toBe("1.3.0");
    expect(lock.packages[""].version).toBe("1.3.0");
    expect(lock.packages["node_modules/some-dep"].version).toBe("1.2.3");

    expect(JSON.parse(readFixture(dir, "src-tauri/tauri.conf.json")).version).toBe("1.3.0");

    const cargoToml = readFixture(dir, "src-tauri/Cargo.toml");
    expect(cargoToml).toContain('name = "vault-buddy"\nversion = "1.3.0"');
    expect(cargoToml).toContain('some-crate = { version = "1.2.3" }');

    const cargoLock = readFixture(dir, "src-tauri/Cargo.lock");
    expect(cargoLock).toContain('name = "vault-buddy"\nversion = "1.3.0"');
    expect(cargoLock).toContain('name = "some-crate"\nversion = "1.2.3"');
  });

  it("computes the next version for patch, minor, and major keywords", () => {
    writeFixtures(dir, "1.2.3");
    expect(runScript(dir, ["patch"]).stdout).toContain("1.2.3 -> 1.2.4");

    writeFixtures(dir, "1.2.3");
    expect(runScript(dir, ["minor"]).stdout).toContain("1.2.3 -> 1.3.0");

    writeFixtures(dir, "1.2.3");
    expect(runScript(dir, ["major"]).stdout).toContain("1.2.3 -> 2.0.0");
  });

  it("rejects an invalid explicit version and writes nothing", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, ["1.2"]);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('Invalid version "1.2"');
    expect(JSON.parse(readFixture(dir, "package.json")).version).toBe("1.2.3");
  });

  it("refuses to bump when the files have already drifted, and writes nothing", () => {
    writeFixtures(dir, "1.2.3");
    const drifted = readFixture(dir, "src-tauri/Cargo.toml").replace(
      'version = "1.2.3"',
      'version = "9.9.9"',
    );
    writeFileSync(path.join(dir, "src-tauri/Cargo.toml"), drifted);

    const result = runScript(dir, ["1.3.0"]);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("Refusing to bump");
    expect(JSON.parse(readFixture(dir, "package.json")).version).toBe("1.2.3");
  });
});
