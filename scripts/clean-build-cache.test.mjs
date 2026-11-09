import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import test from 'node:test';
import { cleanCache } from './clean-build-cache.mjs';

// Keep cleanup tests on the project's drive too.
function fixture(t) {
  const base = path.resolve(import.meta.dirname, '../target');
  fs.mkdirSync(base, { recursive: true });
  const workspace = fs.mkdtempSync(path.join(base, 'cleanup-tests-'));
  t.after(() => {
    assert.equal(path.dirname(fs.realpathSync(workspace)), fs.realpathSync(base));
    fs.rmSync(workspace, { recursive: true, force: true });
  });
  return workspace;
}
function write(root, name) {
  const file = path.join(root, name);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, 'fixture');
  return file;
}

test('cleanup removes only compiler caches and preserves deliverables and validation data', t => {
  const root = fixture(t);
  const deleted = ['target/debug/deps/old.rlib', 'target/debug/incremental/old.o', 'target/release/build/old.obj', 'target/x86_64-pc-windows-msvc/debug/.fingerprint/old', 'target/next-release-validation/debug/deps/old'];
  const kept = ['target/debug/app.exe', 'target/release/bundle/installer.exe', 'target/qr-auth-qa/evidence.json', 'src/main.rs'];
  for (const name of [...deleted, ...kept]) write(root, name);
  assert.equal(cleanCache(root).removed, deleted.length * 7);
  for (const name of deleted) assert.equal(fs.existsSync(path.join(root, name)), false);
  for (const name of kept) assert.equal(fs.existsSync(path.join(root, name)), true);
});

test('automatic cleanup retains small caches; dry-run and busy compiler never delete', t => {
  const root = fixture(t);
  const file = write(root, 'target/debug/incremental/current.o');
  assert.equal(cleanCache(root, { auto: true }).removed, 0);
  assert.equal(cleanCache(root, { dryRun: true }).removed, 7);
  assert.equal(cleanCache(root, { busy: true }).skipped, true);
  assert.ok(fs.existsSync(file));
});

test('cleanup refuses linked cache paths and leaves their destination intact', t => {
  const root = fixture(t);
  const file = write(root, 'outside/sentinel');
  fs.mkdirSync(path.join(root, 'target/debug'), { recursive: true });
  fs.symlinkSync(path.join(root, 'outside'), path.join(root, 'target/debug/deps'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => cleanCache(root), /Linked cache path/);
  assert.ok(fs.existsSync(file));
});
