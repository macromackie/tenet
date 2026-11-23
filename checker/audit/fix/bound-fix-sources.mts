type BoundSourcesInput = Readonly<{
  files: ReadonlyMap<string, string>;
  paths: readonly string[];
  perFileBytes: number;
  totalBytes: number;
}>;

type ClipSourceInput = Readonly<{
  limit: number;
  text: string;
}>;

function sliceAtNewline(text: string): string {
  const newline = text.lastIndexOf('\n');

  if (newline > 0) {
    return text.slice(0, newline);
  }

  return text;
}

function clipSourceText({ limit, text }: ClipSourceInput): string {
  const totalBytes = Buffer.byteLength(text);

  if (totalBytes <= limit) {
    return text;
  }

  const rawBuffer = Buffer.from(text, 'utf-8');
  const sliced = rawBuffer.subarray(0, limit).toString('utf-8');
  const prefix = sliceAtNewline(sliced);
  const shownBytes = Buffer.byteLength(prefix);
  const shown = String(shownBytes);
  const total = String(totalBytes);

  return `${prefix}\n[truncated: showing first ${shown} of ${total} bytes]\n`;
}

/** Serialize a bounded subset of in-memory files for an audit repair prompt payload. */
export function buildBoundedSources({
  files,
  paths,
  perFileBytes,
  totalBytes,
}: BoundSourcesInput): Readonly<Record<string, string>> {
  const entries: [string, string][] = [];
  let remaining = totalBytes;

  for (const path of paths) {
    const raw = files.get(path);

    if (raw !== undefined && remaining > 0) {
      const limit = Math.min(remaining, perFileBytes);
      const bounded = clipSourceText({ limit, text: raw });

      entries.push([path, bounded]);

      remaining -= Buffer.byteLength(bounded);
    }
  }

  // ast-grep-ignore: object-from-entries-typescript -- Prompt payload serializes bounded files as a path-keyed JSON object.
  return Object.fromEntries(entries);
}
