import type { DiskUsageNode } from '../../models';

export type Rgb = readonly [number, number, number];

export type FileTypeGroup =
  | 'video'
  | 'image'
  | 'audio'
  | 'archive'
  | 'binary'
  | 'code'
  | 'document'
  | 'database'
  | 'system'
  | 'other';

export type TreemapColourRole = FileTypeGroup | 'directory' | 'strip';

const GROUP_EXTENSIONS: readonly (readonly [FileTypeGroup, readonly string[]])[] = [
  ['video', ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v', 'wmv', 'flv', 'mpg', 'mpeg', 'mts']],
  [
    'image',
    [
      'jpg',
      'jpeg',
      'png',
      'heic',
      'heif',
      'gif',
      'webp',
      'tif',
      'tiff',
      'raw',
      'cr2',
      'cr3',
      'nef',
      'arw',
      'dng',
      'svg',
      'icns',
      'ico',
      'bmp',
      'psd',
      'avif',
    ],
  ],
  ['audio', ['mp3', 'm4a', 'aac', 'wav', 'flac', 'aiff', 'aif', 'ogg', 'opus', 'wma', 'mid']],
  [
    'archive',
    [
      'zip',
      'tar',
      'gz',
      'tgz',
      'bz2',
      'xz',
      'zst',
      '7z',
      'rar',
      'dmg',
      'pkg',
      'iso',
      'ipa',
      'xip',
      'jar',
      'deb',
      'rpm',
      'msi',
      'cab',
      'nupkg',
      'vsix',
    ],
  ],
  [
    'binary',
    [
      'exe',
      'dll',
      'dylib',
      'so',
      'framework',
      'bin',
      'o',
      'a',
      'lib',
      'obj',
      'metallib',
      'app',
      'wasm',
      'node',
      'class',
      'pyc',
      'rlib',
      'rmeta',
      'pdb',
      'gguf',
      'onnx',
      'safetensors',
    ],
  ],
  [
    'code',
    [
      'ts',
      'tsx',
      'js',
      'jsx',
      'mjs',
      'cjs',
      'rs',
      'go',
      'py',
      'java',
      'kt',
      'swift',
      'c',
      'h',
      'cc',
      'cpp',
      'hpp',
      'm',
      'mm',
      'rb',
      'php',
      'cs',
      'sh',
      'ps1',
      'lua',
      'json',
      'yaml',
      'yml',
      'toml',
      'xml',
      'html',
      'css',
      'scss',
      'vue',
      'sql',
      'map',
    ],
  ],
  [
    'document',
    [
      'pdf',
      'doc',
      'docx',
      'txt',
      'md',
      'rtf',
      'odt',
      'pages',
      'key',
      'ppt',
      'pptx',
      'xls',
      'xlsx',
      'ods',
      'numbers',
      'csv',
      'epub',
      'tex',
    ],
  ],
  ['database', ['db', 'sqlite', 'sqlite3', 'sst', 'wal', 'ldb', 'mdb', 'realm', 'parquet', 'idx']],
  ['system', ['plist', 'log', 'cache', 'dat', 'tmp', 'bak', 'lock', 'pack', 'swp']],
];

const GROUP_BY_EXTENSION = new Map<string, FileTypeGroup>(
  GROUP_EXTENSIONS.flatMap(([group, extensions]) =>
    extensions.map((extension) => [extension, group] as const),
  ),
);

export const TREEMAP_COLOUR_ROLES: readonly TreemapColourRole[] = [
  'video',
  'image',
  'audio',
  'archive',
  'binary',
  'code',
  'document',
  'database',
  'system',
  'other',
  'directory',
  'strip',
];

/** Fallbacks match the light theme and keep the renderer usable without computed styles. */
export const DEFAULT_TREEMAP_COLOURS: Readonly<Record<TreemapColourRole, Rgb>> = {
  video: [242, 128, 56],
  image: [196, 126, 232],
  audio: [96, 196, 112],
  archive: [240, 186, 68],
  binary: [92, 146, 236],
  code: [72, 190, 176],
  document: [140, 170, 222],
  database: [232, 110, 102],
  system: [196, 176, 146],
  other: [170, 162, 150],
  directory: [206, 210, 214],
  strip: [38, 38, 43],
};

/** Lowercased extension without the dot; dotfiles like `.bashrc` have none. */
export function fileExtension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot <= 0 || dot === name.length - 1 ? '' : name.slice(dot + 1).toLowerCase();
}

export function fileTypeGroup(name: string): FileTypeGroup {
  return GROUP_BY_EXTENSION.get(fileExtension(name)) ?? 'other';
}

function hslToRgb(hue: number, saturation: number, lightness: number): Rgb {
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const segment = (hue / 60) % 6;
  const x = chroma * (1 - Math.abs((segment % 2) - 1));
  const [r, g, b] =
    segment < 1
      ? [chroma, x, 0]
      : segment < 2
        ? [x, chroma, 0]
        : segment < 3
          ? [0, chroma, x]
          : segment < 4
            ? [0, x, chroma]
            : segment < 5
              ? [x, 0, chroma]
              : [chroma, 0, x];
  const m = lightness - chroma / 2;
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)] as const;
}

/** Unknown extensions still get distinct, stable colours so a type's tiles read as one group. */
export function hashedExtensionColour(extension: string): Rgb {
  let hash = 2166136261;
  for (let index = 0; index < extension.length; index += 1) {
    hash ^= extension.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hslToRgb((hash >>> 0) % 360, 0.45, 0.62);
}

export type TreemapPalette = Readonly<Record<TreemapColourRole, Rgb>>;

export function nodeColour(node: DiskUsageNode, palette: TreemapPalette): Rgb {
  if (node.kind === 'directory') return palette.directory;
  const extension = fileExtension(node.name);
  const group = GROUP_BY_EXTENSION.get(extension);
  if (group !== undefined) return palette[group];
  return extension === '' ? palette.other : hashedExtensionColour(extension);
}

/** Parses `#rgb`, `#rrggbb` and `rgb()/rgba()` values as produced by computed styles. */
export function parseCssColour(value: string): Rgb | undefined {
  const text = value.trim();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(text)?.[1];
  if (hex !== undefined) {
    const full =
      hex.length === 3
        ? hex
            .split('')
            .map((digit) => digit + digit)
            .join('')
        : hex;
    return [
      Number.parseInt(full.slice(0, 2), 16),
      Number.parseInt(full.slice(2, 4), 16),
      Number.parseInt(full.slice(4, 6), 16),
    ];
  }
  const rgb = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(text);
  if (rgb?.[1] !== undefined && rgb[2] !== undefined && rgb[3] !== undefined) {
    return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])].map((channel) =>
      Math.max(0, Math.min(255, Math.round(channel))),
    ) as unknown as Rgb;
  }
  return undefined;
}

/** Reads the themeable `--fm-disk-usage-*` colours in effect for `element`. */
export function readTreemapPalette(element: Element): TreemapPalette {
  const style = getComputedStyle(element);
  const palette = { ...DEFAULT_TREEMAP_COLOURS };
  for (const role of TREEMAP_COLOUR_ROLES) {
    const parsed = parseCssColour(style.getPropertyValue(`--fm-disk-usage-${role}`));
    if (parsed !== undefined) palette[role] = parsed;
  }
  return palette;
}
