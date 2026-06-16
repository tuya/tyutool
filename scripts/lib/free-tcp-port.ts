import { execSync, spawnSync } from 'node:child_process';

/**
 * Parse Windows `netstat -ano` output for PIDs listening on `port`.
 */
export function parseListeningPidsFromNetstat(output: string, port: number): number[] {
  const pids = new Set<number>();
  const portSuffix = `:${port}`;

  for (const line of output.split(/\r?\n/)) {
    if (!line.includes('LISTENING')) {
      continue;
    }
    const trimmed = line.trim();
    if (!trimmed.includes(portSuffix)) {
      continue;
    }
    const parts = trimmed.split(/\s+/);
    const localAddress = parts[1] ?? '';
    if (!localAddress.endsWith(portSuffix)) {
      continue;
    }
    const pid = Number.parseInt(parts[parts.length - 1] ?? '', 10);
    if (Number.isFinite(pid) && pid > 0) {
      pids.add(pid);
    }
  }

  return [...pids];
}

function commandExists(cmd: string): boolean {
  const r = spawnSync(process.platform === 'win32' ? 'where' : 'which', [cmd], {
    stdio: 'ignore',
  });
  return r.status === 0 && !r.error;
}

function killPids(pids: number[]): void {
  for (const pid of pids) {
    try {
      if (process.platform === 'win32') {
        spawnSync('taskkill', ['/PID', String(pid), '/F'], { stdio: 'ignore' });
      } else {
        process.kill(pid, 'SIGKILL');
      }
    } catch {
      // best-effort
    }
  }
}

function freeTcpPortUnix(port: number): void {
  if (process.platform === 'linux' && commandExists('fuser')) {
    try {
      execSync(`fuser -k ${port}/tcp`, { stdio: 'ignore' });
    } catch {
      // best-effort
    }
  }

  if (!commandExists('lsof')) {
    return;
  }

  try {
    const out = execSync(`lsof -tiTCP:${port} -sTCP:LISTEN`, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    if (!out) {
      return;
    }
    const pids = out
      .split(/\s+/)
      .map((s) => Number.parseInt(s, 10))
      .filter((n) => Number.isFinite(n) && n > 0);
    killPids(pids);
  } catch {
    // best-effort
  }
}

function freeTcpPortWindows(port: number): void {
  try {
    const out = execSync('netstat -ano', {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    const pids = parseListeningPidsFromNetstat(out, port);
    killPids(pids);
  } catch {
    // best-effort
  }
}

/**
 * Release a TCP listen port if occupied (best-effort; never throws).
 */
export function freeTcpPort(port: number): void {
  try {
    if (process.platform === 'win32') {
      freeTcpPortWindows(port);
    } else {
      freeTcpPortUnix(port);
    }
  } catch {
    // best-effort
  }
}
