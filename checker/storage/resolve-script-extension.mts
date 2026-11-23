import type { Check } from '../types.d.mts';

/** Map a checker runtime identifier to its runnable script file extension. */
export function resolveScriptExtension(runtime: Check['runtime']): string {
  switch (runtime) {
    case 'node': {
      return 'mjs';
    }
    case 'python': {
      return 'py';
    }
    case 'go': {
      return 'go';
    }
    case 'rust': {
      return 'rs';
    }
  }
}
