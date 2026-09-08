// Compiles the measured Structured Knowledge Search go decision into a release
// build (task 0208).
//
// The decision lives in the protected repository variable
// `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED`. Only an exact `true` compiles
// `PROCYON_KNOWLEDGE_SEARCH_RELEASE_QUALIFIED` into the binaries; an unset,
// empty, or `false` variable leaves the feature invisible in the produced
// installers. Anything else is a configuration error rather than a silent go,
// because a typo must never qualify a release.
import { appendFileSync } from 'node:fs';

const [environmentFile] = process.argv.slice(2);
if (!environmentFile) {
  throw new Error('usage: export-knowledge-release-qualification.mjs <github-env-file>');
}

const decision = process.env.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED ?? '';
if (decision !== '' && decision !== 'true' && decision !== 'false') {
  throw new Error(
    'KNOWLEDGE_SEARCH_RELEASE_QUALIFIED must be exactly "true" or "false"; ' +
      'an ambiguous value cannot qualify a release',
  );
}

if (decision === 'true') {
  appendFileSync(environmentFile, 'PROCYON_KNOWLEDGE_SEARCH_RELEASE_QUALIFIED=true\n');
  console.log(
    'Structured Knowledge Search is production visible: a measured go decision was recorded.',
  );
} else {
  console.log(
    'Structured Knowledge Search is not production visible: no measured go decision was recorded.',
  );
}
