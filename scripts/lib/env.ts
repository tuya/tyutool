import { homedir } from 'node:os';
import { join } from 'node:path';

const pathSep = process.platform === 'win32' ? ';' : ':';

/**
 * Prepend ~/.cargo/bin to PATH so `cargo` is found in non-login shells.
 */
export function prependCargoBinToPath(): void {
  const cargoBin = join(homedir(), '.cargo', 'bin');
  const cur = process.env.PATH ?? '';
  if (cur.includes(cargoBin)) {
    return;
  }
  process.env.PATH = `${cargoBin}${pathSep}${cur}`;
}
