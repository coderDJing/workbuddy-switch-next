import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const root = path.resolve(import.meta.dirname, '..');
const companion = path.join(root, 'vendor/agent-companion');
const output = path.join(companion, 'dist-embed');
const mode = process.argv[2];

if (!['dev', 'build', 'copy'].includes(mode)) {
  throw new Error('Usage: node scripts/prepare-agent-companion.mjs <dev|build|copy>');
}

const version = JSON.parse(await fs.readFile(path.join(root, 'vendor/agent-companion.version.json'), 'utf8'));
if (!/^[0-9a-f]{40}$/.test(version.revision) || version.interfaceVersion !== 1) {
  throw new Error('Invalid pinned Agent Companion version manifest');
}
console.log(`Agent Companion source: ${version.revision}, host interface: ${version.interfaceVersion}`);

const target = process.env.AGENT_COMPANION_TARGET
  || execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)?.[1];
if (!target) throw new Error('Could not determine Rust target; set AGENT_COMPANION_TARGET');
const suffix = target.includes('windows') ? '.exe' : '';

if (mode !== 'copy') {
  // The archived source carries its own lockfiles, keeping the plugin, UI and
  // runtime on one revision. A clean checkout has no installed dependencies.
  // Windows 的 npm 是 .cmd，Node 20+ 禁止直接 spawn（EINVAL），必须经 shell 解析。
  const shell = process.platform === 'win32';
  execFileSync('npm', ['ci'], { cwd: companion, stdio: 'inherit', shell });
  execFileSync('npm', ['run', 'build:embed'], { cwd: companion, stdio: 'inherit', shell });
  execFileSync('cargo', [
    'build', '--release', '--locked', '--manifest-path', path.join(companion, 'Cargo.toml'),
    '-p', 'agent-studio-runtime', '--target', target,
  ], { cwd: root, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: path.join(root, 'target') } });
  const binaries = path.join(root, 'src-tauri/binaries');
  await fs.mkdir(binaries, { recursive: true });
  const built = path.join(root, 'target', target, 'release', `agent-studio-runtime${suffix}`);
  const packaged = path.join(binaries, `agent-studio-runtime-${target}${suffix}`);
  // 用 rename 而不是 copy：tauri-build 的 copy_binaries 会先把 profile 目录里的同名
  // sidecar 删掉再复制，而在部分 macOS runner 上删刚产出的可执行文件会 EPERM
  // （`fs::remove_file` 直接 unwrap 崩溃）。移走后目标位置不存在，那一步就不会执行。
  // 同一仓库根目录下必然同卷；跨卷或文件被占用时退回复制 + 删除。
  try {
    await fs.rename(built, packaged);
  } catch {
    await fs.copyFile(built, packaged);
    await fs.rm(built, { force: true });
  }
}

for (const page of ['desktop.html', 'desktop-settings.html']) {
  await fs.access(path.join(output, page));
}
if (mode === 'copy' || mode === 'dev') {
  const destination = mode === 'dev'
    ? path.join(root, 'public/companion')
    : path.join(root, 'dist/companion');
  await fs.rm(destination, { recursive: true, force: true });
  await fs.mkdir(path.dirname(destination), { recursive: true });
  await fs.cp(output, destination, { recursive: true });
  console.log(`Agent Companion assets copied to ${path.relative(root, destination)}/`);
}
