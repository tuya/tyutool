import { spawnSync } from 'node:child_process';

export interface RunOptions {
  cwd?: string;
  env?: NodeJS.ProcessEnv;
}

/**
 * Run a command; inherit stdio; exit the process on non-zero status (bash `set -e`).
 */
export function run(cmd: string, args: string[], opts: RunOptions = {}): void {
  const r = spawnSync(cmd, args, {
    stdio: 'inherit',
    cwd: opts.cwd,
    env: opts.env ?? process.env,
    shell: false,
  });
  if (r.error) {
    throw r.error;
  }
  const code = r.status ?? 1;
  if (code !== 0) {
    process.exit(code);
  }
}
