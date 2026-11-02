import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const mode = process.argv[2];
if (!['dev', 'build'].includes(mode)) throw new Error('Usage: node scripts/prepare-cli-sidecar.mjs dev|build');
const target = process.env.AGENT_COMPANION_TARGET || execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)?.[1];
if (!target || !/^[a-z0-9_-]+$/.test(target)) throw new Error('Invalid Rust target');
const release = mode === 'build';
const suffix = target.includes('windows') ? '.exe' : '';
execFileSync('cargo', ['build', '-p', 'wb-switch-server', '--locked', '--target', target, ...(release ? ['--release'] : [])], {
  cwd: root, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: path.join(root, 'target') },
});
const destination = path.join(root, 'src-tauri', 'binaries');
fs.mkdirSync(destination, { recursive: true });
fs.copyFileSync(path.join(root, 'target', target, release ? 'release' : 'debug', `wb-switch${suffix}`),
  path.join(destination, `workbuddy-switch-next-${target}${suffix}`));
console.log(`Prepared desktop CLI for ${target}`);
