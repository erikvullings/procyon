import { defineConfig } from 'vite';

export default defineConfig(({ mode }) => ({
  base: './',
  build: {
    outDir: mode === 'procyon' ? 'dist/procyon' : 'docs',
    emptyOutDir: true
  },
  plugins: mode === 'procyon' ? [{
    name: 'tree-only-procyon',
    transformIndexHtml(html: string) {
      return html.replace(
        /^[ \t]*<script src="https:\/\/cdnjs\.cloudflare\.com\/ajax\/libs\/monaco-editor\/[^"]+"><\/script>\r?\n/m,
        ''
      );
    }
  }] : []
}));
