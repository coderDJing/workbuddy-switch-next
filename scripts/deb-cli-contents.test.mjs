import assert from 'node:assert/strict';
import test from 'node:test';
import { hasExecutableCli } from './deb-cli-contents.mjs';

test('accepts executable CLI with or without a dot prefix', () => {
  for (const prefix of ['', './']) {
    assert.equal(hasExecutableCli(`drwxr-xr-x root/root 0 2026-11-02 00:00 usr/bin/\n-rwxr-xr-x root/root 123 2026-11-02 00:00 ${prefix}usr/bin/workbuddy-switch-next\n`), true);
  }
});

test('rejects a missing, non-executable or different entry', () => {
  for (const line of [
    '',
    '-rw-r--r-- root/root 123 2026-11-02 00:00 usr/bin/workbuddy-switch-next',
    '-rwxr-xr-x root/root 123 2026-11-02 00:00 usr/bin/workbuddy-switch-next-other',
    'lrwxrwxrwx root/root 0 2026-11-02 00:00 usr/bin/workbuddy-switch-next -> elsewhere',
  ]) assert.equal(hasExecutableCli(line), false);
});
