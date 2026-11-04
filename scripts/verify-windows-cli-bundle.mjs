import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const target = process.env.RUST_TARGET;
if (!target || !/^[a-z0-9_-]+$/.test(target) || !target.includes('windows')) throw new Error('Windows RUST_TARGET required');
const bundle = process.env.WB_CLI_BUNDLE_DIR
  ? path.resolve(process.env.WB_CLI_BUNDLE_DIR)
  : path.join(root, 'target', target, 'release', 'bundle', 'nsis');
const installers = fs.readdirSync(bundle).filter(name => name.endsWith('-setup.exe'));
if (installers.length !== 1) throw new Error(`Expected exactly one Windows installer; found ${installers.length}`);
const source = path.join(root, 'src-tauri', 'binaries', `workbuddy-switch-next-${target}.exe`);
const packed = execFileSync(process.env.SEVENZIP || '7z', ['e', '-so', path.join(bundle, installers[0]), 'workbuddy-switch-next.exe'], { maxBuffer: 100 * 1024 * 1024 });
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
if (!packed.length || digest(packed) !== digest(fs.readFileSync(source))) throw new Error('Installer CLI is missing or differs from the built sidecar');
const dailySource = path.join(root, 'src-tauri', 'binaries', `daily-runner-${target}.exe`);
const dailyPacked = execFileSync(process.env.SEVENZIP || '7z', ['e', '-so', path.join(bundle, installers[0]), 'daily-runner.exe'], { maxBuffer: 100 * 1024 * 1024 });
if (!dailyPacked.length || digest(dailyPacked) !== digest(fs.readFileSync(dailySource))) throw new Error('Installer Daily task component is missing or differs from the built sidecar');
console.log('Windows installer contains the exact built CLI sidecar.');
