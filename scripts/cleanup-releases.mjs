// Keeps the public release surface minimal: one current RC, one current stable.
//   RC published     -> remove every superseded RC release + tag, refresh preview feed assets
//   Stable published -> remove every superseded stable release + tag
// Releases of the other channel, drafts, and non RC/stable prereleases are never touched.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { parseReleaseVersion } from './release-policy.mjs';

// The preview channel tag is an updater feed alias, not a version, so it parses to null.
function tagVersion(tag) {
  if (typeof tag !== 'string' || !tag.startsWith('v')) return null;
  try {
    const parsed = parseReleaseVersion(tag.slice(1));
    return { ...parsed, rc: parsed.kind === 'rc' ? tag.split('-rc.')[1] : '' };
  } catch {
    return null;
  }
}

export function tagKind(tag) {
  return tagVersion(tag)?.kind ?? null;
}

function compareRc(a, b) {
  const left = BigInt(a);
  const right = BigInt(b);
  return left < right ? -1 : left > right ? 1 : 0;
}

// Same channel and strictly older than the version being published.
export function isSuperseded(tag, currentTag) {
  const candidate = tagVersion(tag);
  const current = tagVersion(currentTag);
  if (!candidate || !current || candidate.kind !== current.kind) return false;
  for (const part of ['major', 'minor', 'patch']) {
    if (candidate[part] !== current[part]) return candidate[part] < current[part];
  }
  if (candidate.kind === 'stable') return false;
  return compareRc(candidate.rc, current.rc) < 0;
}

// A release is only removable when GitHub itself agrees it is a plain published release
// of the matching channel. Drafts and mislabelled entries (e.g. an RC tagged as stable)
// are left alone so a bad metadata edit never destroys history.
export function isRemovable(release) {
  const kind = tagKind(release.tag_name);
  if (release.draft) return false;
  if (kind === 'rc') return release.prerelease === true;
  if (kind === 'stable') return release.prerelease === false;
  return false;
}

// `refs` are git matching-refs entries, so orphaned tags (no release object) are covered too.
export function cleanupPlan(releases, refs, currentTag) {
  const byTag = new Map(releases.map(release => [release.tag_name, release]));
  const tags = refs
    .map(ref => ref.ref?.replace(/^refs\/tags\//, ''))
    .filter(tag => typeof tag === 'string' && isSuperseded(tag, currentTag))
    .filter(tag => !byTag.has(tag) || isRemovable(byTag.get(tag)));
  const removable = releases.filter(release => isSuperseded(release.tag_name, currentTag) && isRemovable(release));
  return { tags, releases: removable };
}

// Assets present on the feed but absent from the freshly built artifact set are stale.
// An empty artifact list means there is nothing to compare against, so the feed is left intact.
export function staleAssets(feed, expected) {
  if (!expected.length) return [];
  return (feed?.assets ?? []).filter(asset => !expected.includes(asset.name));
}

function gh(...args) {
  return execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
}

function apiJson(path, paginated = false) {
  const data = JSON.parse(gh('api', ...(paginated ? ['--paginate', '--slurp'] : []), path));
  return paginated ? data.flat() : data;
}

function parseArgs(argv) {
  const args = { artifacts: 'artifacts', dryRun: false, apply: false };
  for (let i = 0; i < argv.length; i++) {
    const value = argv[i];
    if (value === '--dry-run') args.dryRun = true;
    else if (value === '--apply') args.apply = true;
    else if (value === '--repo') args.repo = argv[++i];
    else if (value === '--version') args.version = argv[++i];
    else if (value === '--artifacts') args.artifacts = argv[++i];
    else throw new Error(`Unknown argument: ${value}`);
  }
  return args;
}

function deleteRelease(repo, release) {
  gh('api', '--method', 'DELETE', `repos/${repo}/releases/${release.id}`);
}

function deleteTag(repo, tag) {
  gh('api', '--method', 'DELETE', `repos/${repo}/git/refs/tags/${encodeURIComponent(tag)}`);
}

export function planCleanup(repo, version, { artifacts = 'artifacts', sha } = {}) {
  const parsed = parseReleaseVersion(version);
  const currentTag = `v${version}`;
  const releases = apiJson(`repos/${repo}/releases?per_page=100`, true);

  const current = releases.find(release => release.tag_name === currentTag);
  if (!current) throw new Error(`Release ${currentTag} not found; refusing cleanup`);
  if (current.draft) throw new Error(`Release ${currentTag} is still a draft; refusing cleanup`);
  if (parsed.kind === 'rc' ? !current.prerelease : current.prerelease) {
    throw new Error(`Release ${currentTag} is published under the wrong channel; refusing cleanup`);
  }

  // Never delete history when the new release is itself incomplete.
  const expected = existsSync(artifacts) ? readdirSync(artifacts) : [];
  const missing = expected.filter(name => !current.assets.some(asset => asset.name === name));
  if (missing.length) {
    throw new Error(`Release ${currentTag} is missing ${missing.length} built artifact(s); refusing cleanup`);
  }

  let preview = null;
  if (parsed.kind === 'rc') {
    preview = releases.find(release => release.tag_name === 'preview' && release.prerelease && !release.draft);
    if (!preview) throw new Error('preview updater feed is missing; refusing cleanup');
    const previewRef = apiJson(`repos/${repo}/git/ref/tags/preview`);
    if (sha && previewRef.object?.sha !== sha) {
      throw new Error('preview feed does not point at this build; refusing cleanup');
    }
    const missingOnFeed = expected.filter(name => !preview.assets.some(asset => asset.name === name));
    if (missingOnFeed.length) {
      throw new Error(`preview feed is missing ${missingOnFeed.length} built artifact(s); refusing cleanup`);
    }
  }

  const refs = apiJson(`repos/${repo}/git/matching-refs/tags/v`);
  const plan = cleanupPlan(releases, refs, currentTag);
  // A runaway API response must not wipe the release page; this is far above any real history.
  if (plan.releases.length + plan.tags.length > 20) {
    throw new Error(`Cleanup plan too large (${plan.releases.length} releases, ${plan.tags.length} tags); aborting for safety`);
  }
  return { parsed, currentTag, current, preview, expected, plan, stale: staleAssets(preview, expected) };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const repo = args.repo || process.env.GH_REPO;
  const version = args.version || process.env.RELEASE_VERSION;
  if (!repo || !version) throw new Error('Missing repo or version; pass --repo <owner/name> --version <x.y.z[-rc.N]>');

  const { parsed, currentTag, plan, stale } = planCleanup(repo, version, {
    artifacts: args.artifacts,
    sha: process.env.GITHUB_SHA,
  });
  // Local runs default to a preview; CI runs apply immediately after its own build succeeded.
  const dryRun = args.dryRun || (!args.apply && !process.env.GITHUB_ACTIONS);

  console.log(`${parsed.kind === 'rc' ? 'RC' : 'Stable'} ${currentTag} on ${repo}`);
  if (!plan.releases.length && !plan.tags.length && !stale.length) {
    console.log('Nothing to clean up.');
    return;
  }
  for (const release of plan.releases) console.log(`${dryRun ? 'would delete release' : 'deleted release'} ${release.tag_name}`);
  for (const tag of plan.tags) console.log(`${dryRun ? 'would delete tag    ' : 'deleted tag    '} ${tag}`);
  for (const asset of stale) console.log(`${dryRun ? 'would delete asset  ' : 'deleted asset  '} ${asset.name}`);

  if (dryRun) {
    console.log('Dry run: nothing changed. Re-run with --apply to execute.');
    return;
  }
  for (const release of plan.releases) deleteRelease(repo, release);
  for (const tag of plan.tags) deleteTag(repo, tag);
  for (const asset of stale) {
    gh('api', '--method', 'DELETE', `repos/${repo}/releases/assets/${asset.id}`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(`cleanup-releases: ${error.message}`);
    process.exit(1);
  }
}
