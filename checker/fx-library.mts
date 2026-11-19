import { pathToFileURL } from 'node:url';
import type { createFxAgent, memory } from 'libfx';
import { CheckerError } from './checker-error.mts';
import { resolveLibraryPath } from './fx-cache.mts';

type Library = { readonly createFxAgent: typeof createFxAgent; readonly memory: typeof memory };

// ast-grep-ignore: type-predicates-typescript -- libfx exposes JavaScript functions that a JSON schema cannot decode.
function isLibrary(value: unknown): value is Library {
  if (typeof value !== 'object' || value === null) {
    return false;
  }

  if (!('createFxAgent' in value) || typeof value.createFxAgent !== 'function') {
    return false;
  }

  return 'memory' in value && typeof value.memory === 'function';
}

/** Read the authenticated SDK from the pinned host installation. */
export async function readLibrary(): Promise<Library> {
  const trusted = await resolveLibraryPath();
  const url = pathToFileURL(trusted);
  const library: unknown = await import(url.href);

  if (!isLibrary(library)) {
    throw new CheckerError({ message: 'the installed libfx library has no agent runtime' });
  }

  return library;
}
