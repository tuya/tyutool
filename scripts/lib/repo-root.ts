import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Repository root (parent of `scripts/`) from a script module URL.
 */
export function getRepoRoot(importMetaUrl: string): string {
  const dir = dirname(fileURLToPath(importMetaUrl));
  return resolve(dir, '..');
}
