import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const failures = [];

function readText(relativePath) {
  return readFileSync(join(root, relativePath), "utf8");
}

function readJson(relativePath) {
  try {
    return JSON.parse(readText(relativePath));
  } catch (error) {
    failures.push(`${relativePath} is not valid JSON: ${error.message}`);
    return {};
  }
}

function assert(condition, message) {
  if (!condition) failures.push(message);
}

const rootPackage = readJson("package.json");
const desktopPackage = readJson("apps/desktop/package.json");
const tauriConfig = readJson("apps/desktop/src-tauri/tauri.conf.json");
const updaterManifest = readJson("updater/latest.json");
const compatibility = readJson("manifests/compatibility.json");
const compatibilitySchema = readJson("schemas/compatibility.schema.json");
const stackSchema = readJson("schemas/stack.schema.json");
const cargo = readText("Cargo.toml");
const cargoVersion = cargo.match(/\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)?.[1];

const releaseVersions = new Map([
  ["root package", rootPackage.version],
  ["desktop package", desktopPackage.version],
  ["Tauri configuration", tauriConfig.version],
  ["Cargo workspace", cargoVersion],
  ["updater manifest", updaterManifest.version],
]);

for (const [source, version] of releaseVersions) {
  assert(version === rootPackage.version, `${source} version ${version ?? "<missing>"} does not match ${rootPackage.version}`);
}

const updaterKey = readText("updater/public.key").trim();
assert(updaterKey.length > 0, "updater/public.key is empty");
assert(tauriConfig.plugins?.updater?.pubkey === updaterKey, "Tauri updater key does not match updater/public.key");
assert(!existsSync(join(root, "compatibility.json")), "obsolete root compatibility.json must not be restored");

assert(compatibility.$schema === "../schemas/compatibility.schema.json", "compatibility manifest must reference its schema");
assert(compatibility.schemaVersion === 1, "unsupported compatibility schemaVersion");
assert(Array.isArray(compatibility.components) && compatibility.components.length > 0, "compatibility manifest has no components");
assert(Array.isArray(compatibility.artifacts), "compatibility manifest artifacts must be an array");
assert(compatibilitySchema.properties?.components, "compatibility schema does not describe components");
assert(compatibilitySchema.properties?.artifacts, "compatibility schema does not describe artifacts");
assert(stackSchema.properties?.trace, "stack schema does not describe trace configuration");

const componentKinds = new Set();
for (const component of compatibility.components ?? []) {
  assert(!componentKinds.has(component.kind), `duplicate compatibility component: ${component.kind}`);
  componentKinds.add(component.kind);
  assert(/^https:\/\//.test(component.source ?? ""), `${component.kind ?? "component"} source must use HTTPS`);
}

for (const artifact of compatibility.artifacts ?? []) {
  const name = `${artifact.kind ?? "unknown"} ${artifact.version ?? "unknown"}`;
  assert(componentKinds.has(artifact.kind), `${name} has no matching component`);
  assert(/^https:\/\//.test(artifact.url ?? ""), `${name} URL must use HTTPS`);
  assert(/^[0-9a-f]{64}$/.test(artifact.sha256 ?? ""), `${name} SHA-256 is invalid`);
  assert(Number.isSafeInteger(artifact.downloadSize) && artifact.downloadSize > 0, `${name} downloadSize is invalid`);
  assert(
    Number.isSafeInteger(artifact.maximumExtractedSize) && artifact.maximumExtractedSize >= artifact.downloadSize,
    `${name} maximumExtractedSize must be at least downloadSize`,
  );
}

const tracked = spawnSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8" });
assert(tracked.status === 0, `git ls-files failed: ${tracked.stderr.trim()}`);

const forbiddenNames = /(^|\/)(\.env(?:\..+)?|id_rsa|id_ed25519|credentials\.json)$/i;
const secretPatterns = [
  ["private key", /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/],
  ["GitHub token", /\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{30,}\b/],
  ["OpenAI API key", /\bsk-(?:proj-)?[A-Za-z0-9_-]{20,}\b/],
  ["AWS access key", /\bAKIA[0-9A-Z]{16}\b/],
];

for (const relativePath of tracked.stdout.split("\0").filter(Boolean)) {
  const normalized = relativePath.replaceAll("\\", "/");
  assert(!forbiddenNames.test(normalized), `sensitive filename is tracked: ${relativePath}`);
  const absolutePath = join(root, relativePath);
  if (!existsSync(absolutePath)) continue;
  const contents = readFileSync(absolutePath);
  if (contents.length > 2_000_000 || contents.includes(0)) continue;
  const text = contents.toString("utf8");
  for (const [label, pattern] of secretPatterns) {
    assert(!pattern.test(text), `possible ${label} in ${relativePath}`);
  }
}

if (failures.length > 0) {
  console.error("Repository verification failed:\n");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log(`Repository verification passed (${releaseVersions.size} version sources, ${componentKinds.size} compatibility components).`);
