import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { hasExecutableCli } from './deb-cli-contents.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const target = process.env.RUST_TARGET;
const os = process.env.UPDATE_OS;
if (!target || !/^[a-z0-9_-]+$/.test(target) || !['macos', 'linux'].includes(os)) throw new Error('Unix target and UPDATE_OS are required');
const bundle = path.join(root, 'target', target, 'release', 'bundle');
if (os === 'macos') {
  const config = JSON.parse(fs.readFileSync(path.join(root, 'release-config.json'), 'utf8'));
  const packed = path.join(bundle, 'macos', `${config.productName}.app`, 'Contents', 'MacOS', 'workbuddy-switch-next');
  const daily = path.join(path.dirname(packed), 'daily-runner');
  for (const file of [packed, daily]) {
    const stat = fs.statSync(file);
    if (!stat.isFile() || !stat.size || !(stat.mode & 0o111)) throw new Error(`macOS app binary is missing or not executable: ${path.basename(file)}`);
  }
} else {
  const dir = path.join(bundle, 'deb');
  const packages = fs.readdirSync(dir).filter(name => name.endsWith('.deb'));
  if (packages.length !== 1) throw new Error(`Expected one deb package; found ${packages.length}`);
  const contents = execFileSync('dpkg-deb', ['--contents', path.join(dir, packages[0])], { encoding: 'utf8' });
  if (!hasExecutableCli(contents)) throw new Error('Deb package is missing an executable CLI at /usr/bin/workbuddy-switch-next');
  if (!contents.split(/\r?\n/).some(line => /^-[rwxStTs-]{9}\s+.*\s(?:\.\/)?usr\/bin\/daily-runner$/.test(line.trimEnd()) && [line[3], line[6], line[9]].some(permission => /[xst]/.test(permission)))) throw new Error('Deb package is missing an executable Daily task component');
}
console.log(`${os} desktop package contains its CLI.`);
