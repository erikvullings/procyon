import basics from '../../../../docs/walkthroughs/basics.md?raw';
import basket from '../../../../docs/walkthroughs/collection-basket.md?raw';
import finding from '../../../../docs/walkthroughs/finding-files.md?raw';
import remote from '../../../../docs/walkthroughs/remote-transfer.md?raw';
import semantic from '../../../../docs/walkthroughs/semantic-search.md?raw';
import ssh from '../../../../docs/walkthroughs/ssh-sftp.md?raw';
import workspaces from '../../../../docs/walkthroughs/workspaces.md?raw';

export const walkthroughs = [
  { id: 'basics', title: 'Two panes, one safe copy', markdown: basics },
  { id: 'finding-files', title: 'Find a file', markdown: finding },
  { id: 'ssh-sftp', title: 'Connect over SSH/SFTP', markdown: ssh },
  { id: 'remote-transfer', title: 'Transfer files to and from a server', markdown: remote },
  { id: 'semantic-search', title: 'Semantic search and optional answers', markdown: semantic },
  { id: 'collection-basket', title: 'Collect files across folders', markdown: basket },
  { id: 'workspaces', title: 'Organize with workspaces', markdown: workspaces },
] as const;

export type WalkthroughId = (typeof walkthroughs)[number]['id'];

export function walkthroughForHref(href: string): WalkthroughId | undefined {
  const path = href.split(/[?#]/, 1)[0];
  return walkthroughs.find((guide) => `${guide.id}.md` === path)?.id;
}
