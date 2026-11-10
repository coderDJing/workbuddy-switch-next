import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertReleaseChannel } from './release-policy.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = name => fs.readFileSync(path.join(root, name), 'utf8');
const json = name => JSON.parse(read(name));
const release = json('release-config.json');
assertReleaseChannel(release.version, release.channel);
const config = json('src-tauri/tauri.conf.json');
for (const name of ['package.json', 'package-lock.json', 'npm/package.json', 'src-tauri/tauri.conf.json']) {
  assert.equal(json(name).version, release.version, `Version mismatch: ${name}`);
}
for (const name of ['crates/wb-switch-core/Cargo.toml', 'crates/wb-switch-server/Cargo.toml', 'src-tauri/Cargo.toml']) {
  assert.equal(read(name).match(/^version = "([^"]+)"/m)?.[1], release.version, name);
}
assert.equal(config.productName, release.productName);
assert.equal(config.identifier, release.identifier);
assert.equal(json('package.json').name, release.slug);
const npm = json('npm/package.json');
assert.equal(npm.name, release.slug);
assert.equal(npm.bin[release.slug], npm.bin['workbuddy-switch'], 'Legacy command must resolve to the same launcher');
const supported = ['darwin-arm64', 'win32-x64'];
for (const platform of supported) {
  const name = `${release.slug}-${platform}`;
  assert.equal(npm.optionalDependencies[name], release.version);
  const pkg = json(`npm/platform/${name}/package.json`);
  assert.equal(pkg.name, name); assert.equal(pkg.version, release.version);
}
assert.deepEqual(Object.keys(npm.optionalDependencies).sort(), supported.map(platform => `${release.slug}-${platform}`).sort(), 'Only supported platform packages may ship');
const channel = release.channel === 'preview' ? `download/${release.previewTag}` : 'latest/download';
const releasePage = release.channel === 'preview' ? `tag/${release.previewTag}` : 'latest';
assert.ok(read('index.html').includes(`var releaseUrl = "https://github.com/${release.owner}/${release.repo}/releases/${releasePage}";`), 'Startup recovery link must match the release channel');
assert.deepEqual(config.plugins.updater.endpoints, ['latest.json', 'latest-{{target}}-{{arch}}.json']
  .map(name => `https://github.com/${release.owner}/${release.repo}/releases/${channel}/${name}`));
assert.match(Buffer.from(config.plugins.updater.pubkey, 'base64').toString(), /minisign public key/);
const core = read('crates/wb-switch-core/src/modules/update.rs');
for (const [constant, value] of [['GITHUB_OWNER', release.owner], ['GITHUB_REPO', release.repo], ['UPDATE_CHANNEL', release.channel]]) {
  assert.ok(core.includes(`pub const ${constant}: &str = "${value}";`), constant);
}
if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME !== release.previewTag) {
  assert.equal(process.env.GITHUB_REF_NAME, `v${release.version}`, 'Tag must match every shipped manifest');
}
// Platform-gated include_bytes! paths are invisible to a Windows checkout, so a
// renamed icon would only surface on the macOS build. Resolve them here instead.
const embedded = new Set();
const walk = dir => {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(file);
    else if (entry.name.endsWith('.rs')) {
      for (const match of read(path.relative(root, file)).matchAll(/env!\("CARGO_MANIFEST_DIR"\),\s*\n?\s*"([^"]+)"/g)) embedded.add(match[1]);
    }
  }
};
walk(path.join(root, 'src-tauri/src'));
assert.ok(embedded.size > 0, 'Expected embedded icon assets referenced from src-tauri');
for (const relative of embedded) {
  assert.ok(fs.existsSync(path.join(root, 'src-tauri', relative)), `Missing embedded asset: src-tauri${relative}`);
}
console.log(`Release identity and version check passed: ${release.productName} ${release.version} (${release.channel === 'preview' ? 'RC' : 'stable'}).`);
