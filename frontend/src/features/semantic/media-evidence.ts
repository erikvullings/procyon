import { getLocale, t } from '../../i18n';

export type MediaKind = 'image' | 'audio' | 'video';

export function mediaKind(mediaType: string | null | undefined): MediaKind | undefined {
  const type = mediaType?.split(';', 1)[0]?.trim().toLowerCase();
  if (type?.startsWith('image/')) return 'image';
  if (type?.startsWith('audio/')) return 'audio';
  if (type?.startsWith('video/')) return 'video';
  return undefined;
}

export function mediaLabel(kind: MediaKind): string {
  return t(
    'search',
    kind === 'image' ? 'imageResult' : kind === 'audio' ? 'audioResult' : 'videoResult',
  );
}

export function mediaModifiedLabel(value: string | number | null | undefined): string | undefined {
  if (value == null) return undefined;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return undefined;
  return t('search', 'mediaModified', {
    date: new Intl.DateTimeFormat(getLocale(), {
      dateStyle: 'medium',
      timeStyle: 'short',
    }).format(date),
  });
}

export function mediaDescription(text: string): string {
  const excerpt = text.replace(/<\|[^>]*\|>|\[(?:image|audio|video)\]/giu, '').trim();
  return excerpt || t('search', 'mediaInspectSource');
}
