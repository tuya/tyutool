import { getRuntime } from './runtime'

export interface Platform {
  /** Returns absolute local file path, or null if cancelled / unsupported. */
  pickFile(requestId: string, accept: string): Promise<string | null>
  getWsUrl(): string
}

class TauriPlatform implements Platform {
  async pickFile(_requestId: string, accept: string): Promise<string | null> {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const extensions = accept.split(',').map(e => e.trim().replace(/^\./, ''))
    const selected = await open({ multiple: false, filters: [{ name: 'Firmware', extensions }] })
    return selected ?? null
  }
  getWsUrl(): string { return '' }
}

class VscodePlatform implements Platform {
  private readonly _api: { postMessage: (msg: unknown) => void } | null

  constructor() {
    this._api = (window as any).acquireVsCodeApi?.() ?? null
  }

  async pickFile(requestId: string, accept: string): Promise<string | null> {
    if (!this._api) return null
    return new Promise<string | null>(resolve => {
      const timer = setTimeout(() => {
        window.removeEventListener('message', handler)
        resolve(null)
      }, 30_000)

      const handler = (e: MessageEvent) => {
        if (e.data?.type === 'pickFileResult' && e.data.requestId === requestId) {
          clearTimeout(timer)
          window.removeEventListener('message', handler)
          resolve(e.data.path ?? null)
        }
      }
      window.addEventListener('message', handler)
      this._api!.postMessage({ type: 'pickFile', requestId, accept })
    })
  }

  getWsUrl(): string {
    return (window as any).__TUYAOPEN_IDE_CONFIG?.wsUrl ?? ''
  }
}

class WebPlatform implements Platform {
  // Web mode uses a hidden <input type="file"> element triggered by the caller;
  // pickFile() returns null to signal "use DOM input fallback".
  async pickFile(_requestId: string, _accept: string): Promise<string | null> { return null }
  getWsUrl(): string { return '' }
}

export function createPlatform(): Platform {
  const rt = getRuntime()
  if (rt === 'tauri') return new TauriPlatform()
  if (rt === 'vscode') return new VscodePlatform()
  return new WebPlatform()
}

export const platform: Platform = createPlatform()
