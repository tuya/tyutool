import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Find files named `basename` anywhere under `artifacts/` (recursive).
 * Same intent as Python `glob.glob` with `artifacts` + recursive search for that basename.
 */
export function findFilesUnderArtifacts(basename: string): string[] {
  const root = 'artifacts';
  if (!existsSync(root)) {
    return [];
  }
  const out: string[] = [];
  const walk = (dir: string): void => {
    for (const ent of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, ent.name);
      if (ent.isDirectory()) {
        walk(p);
      } else if (ent.name === basename) {
        out.push(p);
      }
    }
  };
  walk(root);
  return out;
}
