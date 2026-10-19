/** The theme currently shown: the explicit `data-theme` choice, else the system preference. */
export function resolvedTheme(): 'light' | 'dark' {
  const theme = document.documentElement.dataset.theme;
  if (theme === 'light' || theme === 'dark') return theme;
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}
