export const FILE_PREVIEW_LIMIT_BYTES = 128 * 1024;

export function shouldFetchPreviewContent(entry: { is_dir: boolean; type: string }) {
  if (entry.is_dir) return false;
  return entry.type === 'text' || entry.type === 'json';
}

export function canStreamPreview(entry: { is_dir: boolean; type: string }) {
  if (entry.is_dir) return false;
  return entry.type === 'image' || entry.type === 'audio' || entry.type === 'video';
}
