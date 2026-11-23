const ASCII_SPACE = 32;
const ASCII_DEL = 127;

/** Replace ASCII control characters with spaces for safe diagnostic previews. */
export function stripControlChars(text: string): string {
  let clean = '';

  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;

    if (code >= ASCII_SPACE && code !== ASCII_DEL) {
      clean += character;
    } else {
      clean += ' ';
    }
  }

  return clean;
}
