import { execFileSync } from 'node:child_process';

/** Verify a PyInstaller executable contains the intended Mach-O architecture. */
export function verifyMacArch(binary, arch) {
  execFileSync('lipo', [binary, '-verify_arch', arch], { stdio: 'pipe' });
}
