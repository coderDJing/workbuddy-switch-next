import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const GiB = 1024 ** 3;
const cacheNames = ['incremental', 'deps', 'build', '.fingerprint'];

// Never traverse links: a cache junction must not turn into deletion elsewhere.
export function directoryBytes(directory) {
  if (!fs.existsSync(directory)) return 0;
  const stat = fs.lstatSync(directory);
  if (stat.isSymbolicLink()) throw new Error(`Linked cache path: ${directory}`);
  if (!stat.isDirectory()) return stat.size;
  return fs.readdirSync(directory).reduce((sum, name) => sum + directoryBytes(path.join(directory, name)), 0);
}

export function cacheDirectories(workspace) {
  const target = path.resolve(workspace, 'target');
  if (!fs.existsSync(target)) return [];
  if (fs.lstatSync(target).isSymbolicLink() || fs.realpathSync(target) !== target) throw new Error('Linked target directory');
  const profiles = [path.join(target, 'debug'), path.join(target, 'release')];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    if (entry.isDirectory() && (/^(?:x86_64|aarch64)-[a-z0-9-]+$/.test(entry.name) || entry.name === 'next-release-validation')) {
      profiles.push(path.join(target, entry.name, 'debug'), path.join(target, entry.name, 'release'));
    }
  }
  const results = [];
  for (const profile of profiles) {
    if (!fs.existsSync(profile)) continue;
    if (fs.realpathSync(profile) !== profile) throw new Error('Linked build profile');
    for (const name of cacheNames) {
      const directory = path.join(profile, name);
      if (fs.existsSync(directory)) results.push({ directory, name, bytes: directoryBytes(directory) });
    }
  }
  return results;
}

export function cleanCache(workspace, { auto = false, dryRun = false, busy = false } = {}) {
  if (busy) return { skipped: true, removed: 0, directories: [] };
  const caches = cacheDirectories(workspace);
  let remaining = caches.reduce((sum, cache) => sum + cache.bytes, 0);
  const incremental = caches.filter(cache => cache.name === 'incremental');
  const incrementalBytes = incremental.reduce((sum, cache) => sum + cache.bytes, 0);
  let selected = caches;
  if (auto) {
    if (remaining <= 30 * GiB && incrementalBytes <= 8 * GiB) return { skipped: false, removed: 0, directories: [] };
    selected = [...incremental];
    remaining -= incrementalBytes;
    if (remaining > 30 * GiB) selected = caches;
  }
  for (const cache of selected) {
    // Revalidate the exact path and its full subtree immediately before removal.
    const target = path.resolve(workspace, 'target');
    if (!cache.directory.startsWith(target + path.sep) || fs.realpathSync(cache.directory) !== cache.directory) throw new Error('Unsafe cache path');
    directoryBytes(cache.directory);
    if (!dryRun) fs.rmSync(cache.directory, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 });
  }
  return { skipped: false, removed: selected.reduce((sum, cache) => sum + cache.bytes, 0), directories: selected.map(cache => cache.directory) };
}

function compilerBusy() {
  // Fail closed when the process inventory cannot be read.
  try {
    const output = process.platform === 'win32'
      ? execFileSync('tasklist.exe', ['/FO', 'CSV', '/NH'], { encoding: 'utf8', windowsHide: true, timeout: 10000 })
      : execFileSync('ps', ['-A', '-o', 'comm='], { encoding: 'utf8', timeout: 10000 });
    return /(?:^|["/\\\s])(?:cargo|rustc|rustdoc)(?:\.exe)?(?:["\s]|$)/im.test(output);
  } catch { return true; }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if (args.some(arg => !['--auto', '--dry-run'].includes(arg))) throw new Error('Usage: clean-build-cache.mjs [--auto] [--dry-run]');
    const result = cleanCache(root, { auto: args.includes('--auto'), dryRun: args.includes('--dry-run'), busy: compilerBusy() });
    if (result.skipped) console.log('Build cache cleanup skipped: compiler active or process inventory unavailable.');
    else {
      for (const directory of result.directories) console.log(`${args.includes('--dry-run') ? 'Would remove' : 'Removed'} ${path.relative(root, directory)}`);
      console.log(`Build cache ${args.includes('--dry-run') ? 'eligible' : 'freed'}: ${(result.removed / GiB).toFixed(2)} GiB`);
    }
  } catch (error) {
    console.error(`Build cache cleanup: ${error.message}`);
    if (!process.argv.includes('--auto')) process.exitCode = 1;
  }
}
