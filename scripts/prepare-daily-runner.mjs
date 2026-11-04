import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import os from 'node:os';
import { verifyMacArch } from './daily-macos-arch.mjs';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const target = process.env.AGENT_COMPANION_TARGET || execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)?.[1];
if (!target || !/^[a-z0-9_-]+$/.test(target)) throw new Error('Invalid Rust target');
const host = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)?.[1];
const crossMac = process.platform === 'darwin' && target.endsWith('-apple-darwin') && host?.endsWith('-apple-darwin');
if (target !== host && !crossMac) throw new Error(`Daily runner requires a matching host (${target}; host ${host})`);
const targetArch = target.startsWith('aarch64-') ? 'arm64' : target.startsWith('x86_64-') ? 'x86_64' : null;
if (crossMac && !targetArch) throw new Error(`Unsupported macOS target ${target}`);
const suffix = process.platform === 'win32' ? '.exe' : '';
const existing = path.join(root, 'src-tauri/binaries', `daily-runner-${target}${suffix}`);
if (process.env.DAILY_RUNNER_PREPARED === '1' && fs.existsSync(existing)) process.exit(0);
const venv = path.join(root, 'target', `daily-python-${target}`);
const python = path.join(venv, process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
const basePython = process.env.DAILY_PYTHON || (process.platform === 'win32' ? 'python' : 'python3');
if (!fs.existsSync(python)) execFileSync(basePython, ['-m', 'venv', venv], { cwd: root, stdio: 'inherit' });
execFileSync(python, ['-m', 'pip', 'install', '--disable-pip-version-check', 'requests==2.32.5', 'pyinstaller==6.16.0'], { cwd: root, stdio: 'inherit' });
const project = path.join(root, 'vendor/daily-runner');
const build = path.join(root, 'target', `daily-runner-build-${target}`);
fs.mkdirSync(build, { recursive: true });
execFileSync(python, ['-m', 'PyInstaller', '--onefile', '--noconfirm', '--clean', '--name', 'daily-runner',
  ...(crossMac ? ['--target-arch', targetArch] : []),
  '--paths', path.join(project, 'upstream/workbuddy'), '--paths', path.join(project, 'upstream/qoder'),
  '--add-data', `${path.join(project, 'upstream/workbuddy/LICENSE')}:licenses/workbuddy`,
  '--add-data', `${path.join(project, 'upstream/workbuddy/NOTICE.md')}:licenses/workbuddy`,
  '--add-data', `${path.join(project, 'upstream/qoder/LICENSE')}:licenses/qoder`,
  '--distpath', build, '--workpath', path.join(build, 'work'), '--specpath', build, path.join(project, 'runner.py')], { cwd: root, stdio: 'inherit' });
const out = path.join(root, 'src-tauri/binaries');
fs.mkdirSync(out, { recursive: true });
const built = path.join(build, `daily-runner${suffix}`);
const stateDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-daily-build-check-'));
try {
  if (target !== host && targetArch === 'arm64' && process.arch === 'x64') {
    verifyMacArch(built, 'arm64');
    console.log('Verified arm64 binary; runtime self-check requires Apple Silicon.');
  } else {
    const check = spawnSync(built, [], { input: JSON.stringify({ mode: 'self-check', stateDir: path.join(stateDir, '每日任务') }) + '\n', encoding: 'utf8', timeout: 30000 });
    if (check.error || check.status !== 0) throw new Error('Daily task component self-check failed');
    if (JSON.parse(check.stdout).ok !== true) throw new Error('Daily task component could not import upstream workflows');
  }
} finally { fs.rmSync(stateDir, { recursive: true, force: true }); }
fs.copyFileSync(built, path.join(out, `daily-runner-${target}${suffix}`));
fs.copyFileSync(built, path.join(out, `daily-runner${suffix}`));
console.log(`Prepared Daily task component for ${target}`);
