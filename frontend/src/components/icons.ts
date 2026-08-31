import m from 'mithril';

/** Common attributes accepted by every icon helper below. */
export interface IconAttrs {
  readonly size?: number;
  readonly className?: string;
}

function icon(viewBox: string, path: string, extraClass: string, attrs: IconAttrs | undefined) {
  const size = attrs?.size ?? 16;
  return m(
    `svg.fm-icon.${extraClass}${attrs?.className === undefined ? '' : `.${attrs.className}`}`,
    {
      'aria-hidden': 'true',
      viewBox,
      width: size,
      height: size,
    },
    m('path', { d: path, fill: 'currentColor' }),
  );
}

/** Material Design "edit" (pencil) glyph, for edit/rename affordances. */
export function editIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    'M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25ZM20.71 7.04a1.003 1.003 0 0 0 0-1.42l-2.34-2.34a1.003 1.003 0 0 0-1.42 0l-1.83 1.83 3.75 3.75 1.84-1.82Z',
    'fm-icon-edit',
    attrs,
  );
}

/** Generic folder glyph, for directory entries. */
export function folderIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    'M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8l-2-2Z',
    'fm-icon-folder',
    attrs,
  );
}

/** Generic document glyph, for file entries. */
export function fileIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    'M6 2a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6H6Zm7 1.5L18.5 9H14a1 1 0 0 1-1-1V3.5Z',
    'fm-icon-file',
    attrs,
  );
}

/** Symlink glyph (a document with a directional arrow), for symbolic links. */
export function symlinkIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    'M6 2a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6H6Zm7 1.5L18.5 9H14a1 1 0 0 1-1-1V3.5ZM8.15 12.5h4.1v-1.65a.35.35 0 0 1 .6-.25l2.55 2.55a.35.35 0 0 1 0 .5l-2.55 2.55a.35.35 0 0 1-.6-.25V14.4h-4.1a.4.4 0 0 1-.4-.4v-1.1a.4.4 0 0 1 .4-.4Z',
    'fm-icon-symlink',
    attrs,
  );
}

/** Shared document silhouette (task 0085 extension-specific badges below reuse this). */
const FILE_BODY_PATH =
  'M6 2a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6H6Zm7 1.5L18.5 9H14a1 1 0 0 1-1-1V3.5Z';

/** Document glyph with a text-lines badge, for PDF/plain-text entries. */
export function pdfIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    `${FILE_BODY_PATH} M8 13h6v1H8Zm0 2h6v1H8Zm0 2h4v1H8Z`,
    'fm-icon-pdf',
    attrs,
  );
}

/** Document glyph with a mountain/sun badge, for image entries. */
export function imageIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    `${FILE_BODY_PATH} M7.5 17.5 9.5 15l1.7 1.9 2.5-2.9 2.8 3.5H7.5Z M9.4 12.6a1.1 1.1 0 1 0 0-2.2 1.1 1.1 0 0 0 0 2.2Z`,
    'fm-icon-image',
    attrs,
  );
}

/** Document glyph with a zipper badge, for archive entries. */
export function archiveIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    `${FILE_BODY_PATH} M11.2 4h1.6v1.4h-1.6Zm0 2.8h1.6v1.4h-1.6Zm0 2.8h1.6v1.4h-1.6Zm0 2.8h1.6v1.4h-1.6Z`,
    'fm-icon-archive',
    attrs,
  );
}

/** Document glyph with a music-note badge, for audio entries. */
export function audioIcon(attrs?: IconAttrs) {
  return icon(
    '0 0 24 24',
    `${FILE_BODY_PATH} M9.6 18.2a1.4 1.4 0 1 0 0-2.8 1.4 1.4 0 0 0 0 2.8Z M10.6 9v7.5h1V9.4l3 .9v-1.1Z`,
    'fm-icon-audio',
    attrs,
  );
}

/** Document glyph with a play-triangle badge, for video entries. */
export function videoIcon(attrs?: IconAttrs) {
  return icon('0 0 24 24', `${FILE_BODY_PATH} M10 10.2 15 13l-5 2.8Z`, 'fm-icon-video', attrs);
}
