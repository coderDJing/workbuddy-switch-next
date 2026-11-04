const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

test('platform installation places the daily component next to the CLI', () => {
  const platform = `${process.platform}-${process.arch}`;
  if (!['darwin-arm64', 'darwin-x64', 'win32-x64', 'linux-x64', 'linux-arm64'].includes(platform)) return;
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-daily-install-'));
  try {
    const script = path.join(root, 'scripts', 'install.js');
    fs.mkdirSync(path.dirname(script), { recursive: true });
    fs.copyFileSync(path.join(__dirname, '../scripts/install.js'), script);
    const pkg = path.join(root, 'node_modules', `workbuddy-switch-next-${platform}`);
    fs.mkdirSync(path.join(pkg, 'bin'), { recursive: true });
    fs.writeFileSync(path.join(pkg, 'package.json'), '{}');
    const suffix = process.platform === 'win32' ? '.exe' : '';
    fs.writeFileSync(path.join(pkg, 'bin', `wb-switch-${platform}${suffix}`), Buffer.alloc(1024 * 1024));
    fs.writeFileSync(path.join(pkg, 'bin', `daily-runner${suffix}`), 'daily fixture');
    const env = { ...process.env };
    delete env.WB_SWITCH_BINARY;
    const result = spawnSync(process.execPath, [script], { env, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(fs.readFileSync(path.join(root, 'bin', `daily-runner${suffix}`), 'utf8'), 'daily fixture');
    if (process.platform !== 'win32') assert.ok(fs.statSync(path.join(root, 'bin', 'daily-runner')).mode & 0o111);
    fs.unlinkSync(path.join(root, 'bin', `daily-runner${suffix}`));
    const override = spawnSync(process.execPath, [script], { env: { ...env, WB_SWITCH_BINARY: path.join(pkg, 'bin', `wb-switch-${platform}${suffix}`) }, encoding: 'utf8' });
    assert.equal(override.status, 0, override.stderr);
    assert.equal(fs.readFileSync(path.join(root, 'bin', `daily-runner${suffix}`), 'utf8'), 'daily fixture');
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
