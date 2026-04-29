import { spawnSync } from 'node:child_process';

import { prependCargoBinToPath } from './env.js';

/**
 * Ensure `cargo` is on PATH (after ~/.cargo/bin); print help and exit if missing.
 */
export function ensureCargoAvailable(): void {
  prependCargoBinToPath();
  const r = spawnSync('cargo', ['--version'], { stdio: 'pipe' });
  if (r.status !== 0 || r.error) {
    console.error('ERROR: cargo not found. Install Rust: https://rustup.rs/');
    console.error(
      'If already installed: export PATH="$HOME/.cargo/bin:$PATH" or source "$HOME/.cargo/env"',
    );
    process.exit(1);
  }
}
