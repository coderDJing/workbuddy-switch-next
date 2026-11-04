import { test } from 'node:test';
import { verifyMacArch } from './daily-macos-arch.mjs';

test('lipo verifies the running Node binary architecture', { skip: process.platform !== 'darwin' }, () => {
  const arch = process.arch === 'arm64' ? 'arm64' : 'x86_64';
  verifyMacArch(process.execPath, arch);
});
