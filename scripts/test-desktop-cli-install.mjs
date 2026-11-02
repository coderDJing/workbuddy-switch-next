import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-cli-install-tests-'));
try {
  const executable = path.join(dir, `cli-install-tests${process.platform === 'win32' ? '.exe' : ''}`);
  execFileSync('rustc', ['--test', '--edition=2021', path.join(root, 'src-tauri/src/cli_install.rs'), '-o', executable], { stdio: 'inherit' });
  execFileSync(executable, [], { stdio: 'inherit' });
} finally {
  if (path.dirname(fs.realpathSync(dir)) !== fs.realpathSync(os.tmpdir()) || !path.basename(dir).startsWith('wb-cli-install-tests-')) throw new Error('Unsafe test directory');
  fs.rmSync(dir, { recursive: true, force: true });
}
