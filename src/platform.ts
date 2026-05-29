import { getRuntime } from './runtime'

export interface PickFileResult {
  path: string
  file: File | null
}

export interface Platform {
  /** Returns picked file info, or null if cancelled / unsupported. */
  pickFile(requestId: string, accept: string): Promise<PickFileResult | null>
  getWsUrl(): string
}

class TauriPlatform implements Platform {
  async pickFile(_requestId: string, accept: string): Promise<PickFileResult | null> {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const extensions = accept.split(',').map(e => e.trim().replace(/^\./, ''))
    const selected = await open({ multiple: false, filters: [{ name: 'Firmware', extensions }] })
    if (!selected) return null
    return { path: selected, file: null }
  }
  getWsUrl(): string { return '' }
}

class VscodePlatform implements Platform {
  private readonly _api: { postMessage: (msg: unknown) => void } | null

  constructor() {
    this._api = (window as any).acquireVsCodeApi?.() ?? null
  }

  async pickFile(requestId: string, accept: string): Promise<PickFileResult | null> {
    if (!this._api) return null
    return new Promise<PickFileResult | null>(resolve => {
      const timer = setTimeout(() => {
        window.removeEventListener('message', handler)
        resolve(null)
      }, 30_000)

      const handler = (e: MessageEvent) => {
        if (e.data?.type === 'pickFileResult' && e.data.requestId === requestId) {
          clearTimeout(timer)
          window.removeEventListener('message', handler)
          const path: string | null = e.data.path ?? null
          const content: string | null = e.data.content ?? null
          if (!path) { resolve(null); return }
          let file: File | null = null
          if (content) {
            // Extension sends file bytes as base64; create a File so ws-transport
            // can encode it as file content when sending to tyutool_cli.
            const bytes = Uint8Array.from(atob(content), c => c.charCodeAt(0))
            const name = path.split(/[\\/]/).pop() ?? 'firmware.bin'
            file = new File([bytes], name)
          }
          resolve({ path, file })
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
  async pickFile(_requestId: string, _accept: string): Promise<PickFileResult | null> { return null }
  getWsUrl(): string { return '' }
}

export function createPlatform(): Platform {
  const rt = getRuntime()
  if (rt === 'tauri') return new TauriPlatform()
  if (rt === 'vscode') return new VscodePlatform()
  return new WebPlatform()
}

export const platform: Platform = createPlatform()
