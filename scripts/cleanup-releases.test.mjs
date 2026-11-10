import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cleanupPlan, isRemovable, isSuperseded, staleAssets, tagKind } from './cleanup-releases.mjs';

test('RC tags only outrank older RCs of the same channel', () => {
  const current = 'v0.2.0-rc.202611020103';
  for (const tag of ['v0.2.0-rc.3', 'v0.1.9-rc.202611020104']) assert.equal(isSuperseded(tag, current), true);
  for (const tag of [
    current,
    'v0.2.0-rc.202611020104',
    'v0.3.0-rc.1',
    'v0.1.9',
    'v0.2.0',
    'preview',
    'v0.2.0-beta.1',
    'v0.2.0-rc.01',
  ]) assert.equal(isSuperseded(tag, current), false);
});

test('stable tags only outrank older stable releases', () => {
  const current = 'v0.2.6';
  for (const tag of ['v0.2.0', 'v0.2.5', 'v0.1.56']) assert.equal(isSuperseded(tag, current), true);
  for (const tag of [current, 'v0.2.7', 'v0.2.7-rc.202611101801', 'v0.2.6-rc.202611101404', 'preview']) {
    assert.equal(isSuperseded(tag, current), false);
  }
});

test('publishing a stable never targets RC history and vice versa', () => {
  const releases = [
    { tag_name: 'v0.2.5', prerelease: false, draft: false },
    { tag_name: 'v0.2.7-rc.202611101801', prerelease: true, draft: false },
  ];
  const refs = ['v0.2.5', 'v0.2.7-rc.202611101801'].map(tag => ({ ref: `refs/tags/${tag}` }));

  const stablePlan = cleanupPlan(releases, refs, 'v0.2.6');
  assert.deepEqual(stablePlan.releases.map(r => r.tag_name), ['v0.2.5']);
  assert.deepEqual(stablePlan.tags, ['v0.2.5']);

  const rcPlan = cleanupPlan(releases, refs, 'v0.2.8-rc.202611102300');
  assert.deepEqual(rcPlan.releases.map(r => r.tag_name), ['v0.2.7-rc.202611101801']);
  assert.deepEqual(rcPlan.tags, ['v0.2.7-rc.202611101801']);
});

test('cleanup includes orphaned tags but protects drafts and mislabelled releases', () => {
  const releases = [
    { tag_name: 'v0.2.0-rc.1', prerelease: true, draft: false },
    { tag_name: 'v0.2.0-rc.2', prerelease: true, draft: true },
    { tag_name: 'v0.2.0-rc.3', prerelease: false, draft: false },
    { tag_name: 'preview', prerelease: true, draft: false },
    { tag_name: 'v0.1.9-rc.1', prerelease: true, draft: false },
  ];
  const refs = ['v0.2.0-rc.1', 'v0.2.0-rc.2', 'v0.2.0-rc.3', 'v0.2.0-rc.4', 'v0.2.0-rc.5', 'v0.2.0']
    .map(tag => ({ ref: `refs/tags/${tag}` }));
  const plan = cleanupPlan(releases, refs, 'v0.2.0-rc.5');
  // rc.2 is a draft and rc.3 is tagged stable, so their tags stay; rc.4 has no release at all.
  assert.deepEqual(plan.tags, ['v0.2.0-rc.1', 'v0.2.0-rc.4']);
  assert.deepEqual(plan.releases.map(r => r.tag_name), ['v0.2.0-rc.1', 'v0.1.9-rc.1']);
});

test('draft stable releases and prereleases are not removable as stable', () => {
  assert.equal(isRemovable({ tag_name: 'v0.2.5', prerelease: false, draft: true }), false);
  assert.equal(isRemovable({ tag_name: 'v0.2.5', prerelease: true, draft: false }), false);
  assert.equal(isRemovable({ tag_name: 'v0.2.5', prerelease: false, draft: false }), true);
  assert.equal(isRemovable({ tag_name: 'preview', prerelease: true, draft: false }), false);
});

test('channel detection ignores non version tags', () => {
  assert.equal(tagKind('preview'), null);
  assert.equal(tagKind('v0.2.0-beta.1'), null);
  assert.equal(tagKind('v0.2.6'), 'stable');
  assert.equal(tagKind('v0.2.7-rc.202611101801'), 'rc');
});

test('stale feed assets are the ones no longer produced', () => {
  const feed = { assets: [{ name: 'a', id: 1 }, { name: 'b', id: 2 }, { name: 'old', id: 3 }] };
  assert.deepEqual(staleAssets(feed, ['a', 'b']).map(asset => asset.name), ['old']);
  assert.deepEqual(staleAssets(feed, ['a', 'b', 'old']), []);
});

test('an empty artifact list never wipes the feed', () => {
  const feed = { assets: [{ name: 'a', id: 1 }, { name: 'b', id: 2 }] };
  assert.deepEqual(staleAssets(feed, []), []);
  assert.deepEqual(staleAssets(null, ['a']), []);
});
