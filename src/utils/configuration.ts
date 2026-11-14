import { fs } from '@tauri-apps/api'
import { dataDir } from '@tauri-apps/api/path'
import { invoke } from '@tauri-apps/api/tauri'

export interface Configuration {
  game_install_path: string
  last_ip: string
  last_port: string
  language: string
  cert_generated: boolean
  debug_enabled: boolean
  patch_rsa: boolean
  use_internal_proxy: boolean
  wipe_login: boolean
  un_elevated: boolean
  launch_args: string
  show_version: boolean
  gimi_enabled: boolean
  gimi_mods_path: string
  gimi_fps: number
}

const defaults: Configuration = {
  game_install_path: '',
  last_ip: 'localhost',
  last_port: '443',
  language: 'chs',
  cert_generated: false,
  debug_enabled: false,
  patch_rsa: true,
  use_internal_proxy: true,
  wipe_login: false,
  un_elevated: false,
  launch_args: '',
  show_version: true,
  gimi_enabled: false,
  gimi_mods_path: '',
  gimi_fps: 0,
}

let filePath = ''
let loaded: Promise<void> | undefined
let current: Configuration | undefined
let writes: Promise<void> = Promise.resolve()
const listeners = new Set<(config: Configuration, previous?: Configuration) => void>()

function clone(config: Configuration): Configuration {
  return { ...config }
}

function normalize(value: unknown): Configuration {
  const source = (value && typeof value === 'object' && !Array.isArray(value) ? value : {}) as Partial<Configuration>
  const result = { ...defaults }
  for (const key of Object.keys(defaults) as (keyof Configuration)[]) {
    if (typeof source[key] === typeof defaults[key]) {
      ;(result as Record<string, unknown>)[key] = source[key]
    }
  }
  if (!Number.isInteger(result.gimi_fps) || (result.gimi_fps !== 0 && (result.gimi_fps < 10 || result.gimi_fps > 1000))) {
    result.gimi_fps = defaults.gimi_fps
  }
  return result
}

async function load(): Promise<void> {
  if (!loaded) {
    loaded = (async () => {
      filePath = (await dataDir()) + 'cultivation/configuration.json'
      if (!(await invoke<boolean>('does_file_exist', { path1: filePath }))) {
        current = clone(defaults)
        return
      }
      const raw = await fs.readTextFile(filePath)
      try {
        current = normalize(JSON.parse(raw))
      } catch (error) {
        console.error('Invalid configuration, using defaults:', error)
        await fs.copyFile(filePath, filePath + '.corrupt-' + Date.now())
        current = clone(defaults)
      }
    })().catch((error) => {
      loaded = undefined
      throw error
    })
  }
  await loaded
}

function enqueue(action: () => Promise<void>): Promise<void> {
  const pending = writes.then(action)
  writes = pending.catch(() => undefined)
  return pending
}

function notify(config: Configuration, previous?: Configuration) {
  listeners.forEach((listener) => {
    try {
      listener(clone(config), previous && clone(previous))
    } catch (error) {
      console.error('Configuration listener failed:', error)
    }
  })
}

async function persist(next: Configuration): Promise<void> {
  const temporary = filePath + '.tmp'
  await fs.createDir(filePath.slice(0, filePath.lastIndexOf('/')), { recursive: true })
  await fs.writeFile({ path: temporary, contents: JSON.stringify(next) })
  await fs.renameFile(temporary, filePath)
  const previous = current
  current = clone(next)
  notify(current, previous)
}

export function subscribeConfig(listener: (config: Configuration, previous?: Configuration) => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export async function getConfig(): Promise<Configuration> {
  await load()
  await writes
  return clone(current as Configuration)
}

export async function getConfigOption<K extends keyof Configuration>(key: K): Promise<Configuration[K]> {
  return (await getConfig())[key]
}

export function updateConfig(patch: Partial<Configuration>): Promise<void> {
  const snapshot = { ...patch }
  return enqueue(async () => {
    await load()
    await persist(normalize({ ...(current as Configuration), ...snapshot }))
  })
}

export function setConfigOption<K extends keyof Configuration>(key: K, value: Configuration[K]): Promise<void> {
  return updateConfig({ [key]: value } as Partial<Configuration>)
}
