import { invoke } from '@tauri-apps/api'
import { getConfig } from './configuration'

export async function getGameExecutable() {
  const config = await getConfig()

  if (!config.game_install_path) {
    return null
  }

  const pathArr = config.game_install_path.replace(/\\/g, '/').split('/')
  return pathArr[pathArr.length - 1]
}

export async function getGameVersion(path?: string): Promise<{ major: number; minor: number; release: number } | null> {
  const selected = path ?? (await getConfig()).game_install_path
  if (!selected) return null
  try {
    return await invoke('get_game_version', { path: selected })
  } catch (error) {
    console.error('Unable to determine game version:', error)
    return null
  }
}
