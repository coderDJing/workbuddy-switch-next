import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { buildManifest, collect, desktopReleaseNotes, manifestFromEnvironment, platformSpec, validateArtifacts } from './release-artifacts.mjs';

const release = { owner: 'coderDJing', repo: 'workbuddy-switch-next', slug: 'workbuddy-switch-next', version: '0.2.0-rc.1', channel: 'preview' };
test('release notes lead with two immutable desktop downloads and are idempotent', () => {
  for (const config of [release, { ...release, version: '0.2.0', channel: 'stable' }]) {
    const data = { ...config, slug: 'workbuddy-switch-next' };
    const notes = desktopReleaseNotes('# Changes\n\nKeep existing notes.\n', data);
    assert.equal((notes.match(/\]\(https:/g) || []).length, 2);
    assert.ok(notes.indexOf('下载 Windows') < notes.indexOf('# Changes'));
    assert.ok(notes.includes(`/download/v${config.version}/workbuddy-switch-next-windows-x86_64-setup.exe`));
    assert.ok(notes.includes(`/download/v${config.version}/workbuddy-switch-next-macos-aarch64.dmg`));
    assert.ok(notes.endsWith('# Changes\n\nKeep existing notes.\n'));
    assert.equal(desktopReleaseNotes(notes, data), notes);
  }
});

test('collection publishes desktop archives and signatures without standalone components', t => {
  const base = path.resolve(import.meta.dirname, '../target');
  fs.mkdirSync(base, { recursive: true });
  const workspace = fs.mkdtempSync(path.join(base, 'release-collect-test-'));
  t.after(() => fs.rmSync(workspace, { recursive: true, force: true }));
  const data = { ...release, slug: 'workbuddy-switch-next' };
  for (const [target, platform, os, arch, folder, name] of [
    ['x86_64-pc-windows-msvc', 'win32-x64', 'windows', 'x86_64', 'nsis', `WorkBuddy Switch Next_${release.version}_x64-setup.exe`],
    ['aarch64-apple-darwin', 'darwin-arm64', 'macos', 'aarch64', 'macos', 'WorkBuddy Switch Next.app.tar.gz'],
  ]) {
    const bundle = path.join(workspace, 'target', target, 'release/bundle', folder);
    fs.mkdirSync(bundle, { recursive: true });
    fs.writeFileSync(path.join(bundle, name), 'desktop fixture');
    fs.writeFileSync(path.join(bundle, name + '.sig'), 'signature fixture');
    if (os === 'macos') fs.writeFileSync(path.join(workspace, `${data.slug}_${data.version}_${arch}.dmg`), 'disk image fixture');
    collect({ RUST_TARGET: target, PLATFORM_TAG: platform, UPDATE_OS: os, UPDATE_ARCH: arch }, { workspace, release: data });
  }
  const files = fs.readdirSync(path.join(workspace, 'dist-bin'));
  assert.equal(files.length, 7);
  assert.equal(files.filter(name => name.includes('-cli-') || name.includes('-daily-runner-')).length, 0);
  assert.ok(files.includes('workbuddy-switch-next-windows-x86_64-setup.exe'));
  assert.ok(files.includes('workbuddy-switch-next-macos-aarch64.dmg'));
  assert.equal(fs.existsSync(path.join(workspace, 'npm')), false);
});
test('RC manifests pin immutable signed archives and safely encode filenames', () => {
  const data = buildManifest({ release, archiveName: 'WorkBuddy Switch Next_0.2.0-rc.1_x64-setup.exe', signature: 'fixture\n', keys: ['windows-x86_64-nsis'] });
  assert.equal(data.version, release.version);
  assert.equal(data.platforms['windows-x86_64-nsis'].signature, 'fixture');
  assert.equal(data.platforms['windows-x86_64-nsis'].url,
    'https://github.com/coderDJing/workbuddy-switch-next/releases/download/v0.2.0-rc.1/WorkBuddy%20Switch%20Next_0.2.0-rc.1_x64-setup.exe');
  assert.throws(() => buildManifest({ release, archiveName: '../other.exe', signature: 'x', keys: ['win'] }));
});
test('release artifacts only support Windows x64 and Apple Silicon', () => {
  assert.deepEqual(platformSpec('macos', 'aarch64').keys, ['darwin-aarch64']);
  assert.deepEqual(platformSpec('windows', 'x86_64').keys, ['windows-x86_64-nsis', 'windows-x86_64']);
  for (const [os, arch, bundle] of [['linux', 'x86_64', 'deb'], ['linux', 'x86_64', 'appimage'], ['linux', 'aarch64', ''], ['macos', 'x86_64', ''], ['windows', 'aarch64', '']]) {
    assert.throws(() => platformSpec(os, arch, bundle), /Unsupported platform/);
  }
});

test('artifact entry points reject unsupported versions and channel mismatches before filesystem work', () => {
  const input = { archiveName: 'installer.exe', signature: 'fixture', keys: ['windows-x86_64'] };
  for (const version of ['0.2.0-beta.1', '0.2.0-alpha.1', '0.2.0-rc.0', '00.2.0', '0.2.0-rc.01']) {
    const invalid = { ...release, version };
    assert.throws(() => buildManifest({ ...input, release: invalid }), /Unsupported release version/);
    assert.throws(() => manifestFromEnvironment({ BUNDLE_DIR: 'unused', UPDATE_VERSION: version }, release), /Unsupported release version/);
    assert.throws(() => validateArtifacts('unused', invalid), /Unsupported release version/);
  }
  assert.throws(() => buildManifest({ ...input, release: { ...release, channel: 'stable' } }), /channel mismatch/);
  const stable = buildManifest({ ...input, release: { ...release, version: '0.2.0', channel: 'stable' } });
  assert.equal(stable.version, '0.2.0');
  assert.match(stable.platforms['windows-x86_64'].url, /\/v0\.2\.0\/installer\.exe$/);
});
test('manifest generation rejects unsigned or ambiguous Windows installers', t => {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-next-manifest-'));
  t.after(() => fs.rmSync(folder, { recursive: true, force: true }));
  const archive = path.join(folder, 'WorkBuddy Switch Next_0.2.0-rc.1_x64-setup.exe');
  fs.writeFileSync(archive, 'isolated installer fixture');
  const env = { BUNDLE_DIR: folder, UPDATE_OS: 'windows', UPDATE_ARCH: 'x86_64' };
  assert.throws(() => manifestFromEnvironment(env, release), /Missing updater signature/);
  fs.writeFileSync(archive + '.sig', 'signature fixture');
  const file = manifestFromEnvironment(env, release);
  const data = JSON.parse(fs.readFileSync(file, 'utf8'));
  assert.deepEqual(Object.keys(data.platforms), ['windows-x86_64-nsis', 'windows-x86_64']);
  fs.writeFileSync(path.join(folder, 'other_0.2.0-rc.1_x64-setup.exe'), 'other installer');
  assert.throws(() => manifestFromEnvironment(env, release), /found 2/);
});

test('publication rejects mismatched versions, missing archives and substituted signatures', t => {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-next-validate-'));
  t.after(() => fs.rmSync(folder, { recursive: true, force: true }));
  const name = 'next-windows-setup.exe';
  fs.writeFileSync(path.join(folder, name), 'installer fixture');
  fs.writeFileSync(path.join(folder, name + '.sig'), 'signature fixture');
  const data = buildManifest({ release, archiveName: name, signature: 'signature fixture', keys: ['windows-x86_64'] });
  const manifest = path.join(folder, 'latest.json');
  fs.writeFileSync(manifest, JSON.stringify(data));
  assert.equal(validateArtifacts(folder, release), 1);
  const standalone = path.join(folder, `${release.slug}-daily-runner-win32-x64.exe`);
  fs.writeFileSync(standalone, 'standalone fixture');
  assert.throws(() => validateArtifacts(folder, release), /must stay inside desktop packages/);
  fs.unlinkSync(standalone);
  data.version = '0.2.0-rc.2'; fs.writeFileSync(manifest, JSON.stringify(data));
  assert.throws(() => validateArtifacts(folder, release), /Version mismatch/);
  data.version = release.version; data.platforms['windows-x86_64'].signature = 'other';
  fs.writeFileSync(manifest, JSON.stringify(data));
  assert.throws(() => validateArtifacts(folder, release), /Signature mismatch/);
});
