#!/usr/bin/env node
// Decides whether a push to main may reuse the CI result of its pull request.
// Reuse is allowed only when the pushed tree is byte-for-byte the tree that a
// successful pull-request CI run tested, and every job in that run succeeded
// (none skipped by path filters). Any doubt or error falls back to full CI.
import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TESTED_TREE_ARTIFACT = 'ci-tested-tree';
export const TESTED_TREE_FILE = 'tree.txt';

export async function decideReuse({ sha, tree, api }) {
  const pulls = await api.pullsForCommit(sha);
  const pull = pulls.find((candidate) => candidate.merge_commit_sha === sha && candidate.merged_at);
  if (pull === undefined) return { reuse: false, reason: 'no merged pull request for this commit' };

  const runs = await api.successfulPullRequestRuns(pull.head.sha);
  for (const run of runs) {
    const jobs = await api.jobsForRun(run.id);
    if (jobs.length === 0 || jobs.some((job) => job.conclusion !== 'success')) continue;
    const testedTree = await api.testedTreeForRun(run.id);
    if (testedTree === tree) {
      return { reuse: true, reason: `tree ${tree} passed in PR #${pull.number} run ${run.id}` };
    }
  }
  return { reuse: false, reason: `no fully successful PR #${pull.number} run tested tree ${tree}` };
}

function gh(args) {
  return execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

function githubApi(repo) {
  const getJson = (path) => JSON.parse(gh(['api', path]));
  return {
    pullsForCommit: (sha) => getJson(`repos/${repo}/commits/${sha}/pulls`),
    successfulPullRequestRuns: (headSha) =>
      getJson(
        `repos/${repo}/actions/workflows/ci.yml/runs?event=pull_request&status=success&head_sha=${headSha}&per_page=20`,
      ).workflow_runs,
    jobsForRun: (runId) =>
      getJson(`repos/${repo}/actions/runs/${runId}/jobs?filter=latest&per_page=100`).jobs,
    testedTreeForRun: (runId) => {
      const dir = mkdtempSync(join(tmpdir(), 'ci-tested-tree-'));
      try {
        gh(['run', 'download', String(runId), '-R', repo, '-n', TESTED_TREE_ARTIFACT, '-D', dir]);
        return readFileSync(join(dir, TESTED_TREE_FILE), 'utf8').trim();
      } catch {
        return undefined;
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    },
  };
}

async function main() {
  const { GITHUB_REPOSITORY: repo, GITHUB_SHA: sha, GITHUB_OUTPUT: output } = process.env;
  let decision;
  try {
    const tree = execFileSync('git', ['rev-parse', 'HEAD^{tree}'], { encoding: 'utf8' }).trim();
    decision = await decideReuse({ sha, tree, api: githubApi(repo) });
  } catch (error) {
    decision = { reuse: false, reason: `gate error, running full CI: ${error.message}` };
  }
  console.log(`${decision.reuse ? 'Reusing' : 'Not reusing'} PR CI: ${decision.reason}`);
  if (output) appendFileSync(output, `reuse=${decision.reuse}\n`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
