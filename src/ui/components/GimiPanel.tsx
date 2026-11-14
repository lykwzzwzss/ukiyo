import React, { useCallback, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { getConfig, setConfigOption, subscribeConfig } from '../../utils/configuration'
import { alertDialog } from './common/Dialog'
import './GimiPanel.css'

interface ModEntry {
  folder: string
  name: string
  displayName?: string
  enabled: boolean
  iniCount: number
  compatibility: string
}

interface GimiStatus {
  version: string
  modsPath: string
  enabled: boolean
  fps: number
  mods: ModEntry[]
  running: boolean
  pendingChanges: number
}

interface IProps {
  onClose: () => void
  visible: boolean
}

export default function GimiPanel({ onClose, visible }: IProps) {
  const [status, setStatus] = useState<GimiStatus | null>(null)
  const [query, setQuery] = useState('')
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)
  const mounted = useRef(true)
  const request = useRef(0)
  const visibleRef = useRef(visible)
  visibleRef.current = visible
  const invalidated = useRef(false)
  const [fpsStatus, setFpsStatus] = useState('')
  const [error, setError] = useState('')
  const [warming, setWarming] = useState(false)
  const [editing, setEditing] = useState<{ folder: string; value: string; error: string } | null>(null)
  const nameInput = useRef<HTMLInputElement>(null)

  useEffect(() => {
    let stopped = false
    let generation = 0
    let timer: ReturnType<typeof setTimeout> | undefined
    const schedule = async () => {
      const current = ++generation
      if (timer !== undefined) clearTimeout(timer)
      const config = await getConfig()
      if (stopped || current !== generation) return
      setWarming(false)
      if (!config.gimi_enabled || !config.game_install_path) return
      timer = setTimeout(() => {
        if (stopped || current !== generation) return
        setWarming(true)
        void invoke<boolean>('gimi_preload', { game: config.game_install_path })
          .catch((reason) => console.error('GIMI background preparation failed; game launch will retry:', reason))
          .finally(() => { if (!stopped && current === generation) setWarming(false) })
      }, 250)
    }
    void schedule().catch(console.error)
    const unsubscribe = subscribeConfig((config, previous) => {
      if (!previous || config.game_install_path !== previous.game_install_path || config.gimi_mods_path !== previous.gimi_mods_path || config.gimi_enabled !== previous.gimi_enabled) void schedule().catch(console.error)
    })
    let unlisten: (() => void) | undefined
    void listen<boolean>('game_state_changed', (event) => {
      if (!event.payload) void schedule().catch(console.error)
    }).then((off) => { if (stopped) off(); else unlisten = off }).catch(console.error)
    return () => { stopped = true; generation++; if (timer !== undefined) clearTimeout(timer); unsubscribe(); unlisten?.() }
  }, [])

  const load = useCallback(async (mode: 'status' | 'refresh' | 'reload' = 'status') => {
    const generation = ++request.current
    try {
      const next = await invoke<GimiStatus>(mode === 'reload' ? 'gimi_reload' : mode === 'refresh' ? 'gimi_refresh' : 'gimi_status')
      if (!mounted.current || generation !== request.current) return
      setStatus(next)
      setError('')
    } catch (reason) {
      if (!mounted.current || generation !== request.current) return
      const message = String(reason)
      setError(message)
      if (visibleRef.current) await alertDialog({ title: '模型库', message })
    }
  }, [])

  useEffect(() => {
    mounted.current = true
    void load()
    const update = () => {
      if (!mounted.current) return
      if (busyRef.current) invalidated.current = true
      else void load()
    }
    const unsubscribe = subscribeConfig((config, previous) => {
      if (previous && (config.game_install_path !== previous.game_install_path || config.gimi_mods_path !== previous.gimi_mods_path)) update()
      else setStatus((current) => current && !current.running && (current.enabled !== config.gimi_enabled || current.fps !== config.gimi_fps)
        ? { ...current, enabled: config.gimi_enabled, fps: config.gimi_fps } : current)
    })
    let unlisten: (() => void) | undefined
    void listen<boolean>('game_state_changed', () => update()).then((off) => {
      if (mounted.current) unlisten = off
      else off()
    }).catch(console.error)
    return () => { mounted.current = false; request.current++; unsubscribe(); unlisten?.() }
  }, [load])

  useEffect(() => {
    if (!visible) return
    let checking = false
    let stopped = false
    const check = async () => {
      if (checking || document.hidden) return
      checking = true
      try {
        const [running, message] = await Promise.all([invoke<boolean>('is_game_running'), invoke<string>('gimi_fps_status')])
        if (mounted.current && !stopped && visibleRef.current) {
          setStatus((current) => current && current.running !== running ? { ...current, running } : current)
          setFpsStatus(message)
        }
      } catch (reason) { console.error(reason) }
      finally { checking = false }
    }
    void check()
    const timer = window.setInterval(() => void check(), 2000)
    const resume = () => { if (!document.hidden) void check() }
    document.addEventListener('visibilitychange', resume)
    return () => { stopped = true; window.clearInterval(timer); document.removeEventListener('visibilitychange', resume) }
  }, [visible])

  useEffect(() => {
    if (!visible) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented) onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose, visible])

  useEffect(() => {
    if (!visible) setEditing(null)
  }, [visible])

  useEffect(() => {
    if (editing) { nameInput.current?.focus({ preventScroll: true }); nameInput.current?.select() }
  }, [editing?.folder])

  const finishUpdate = () => {
    busyRef.current = false
    if (!mounted.current) return
    setBusy(false)
    if (invalidated.current) { invalidated.current = false; void load() }
  }

  const rename = async () => {
    if (!editing || busyRef.current) return
    const value = editing.value.trim()
    if (!value) { setEditing({ ...editing, error: '名称不能为空' }); return }
    const mod = status?.mods.find((item) => item.folder === editing.folder)
    if (!mod || value === (mod.displayName || mod.name)) { setEditing(null); return }
    busyRef.current = true
    setBusy(true)
    try {
      const displayName = await invoke<string>('gimi_rename_mod', { folder: editing.folder, name: value })
      if (mounted.current) {
        setStatus((current) => current ? { ...current, mods: current.mods.map((item) => item.folder === editing.folder ? { ...item, displayName } : item) } : current)
        setEditing(null)
      }
    } catch (reason) {
      if (mounted.current && visibleRef.current) {
        setEditing({ ...editing, error: String(reason) })
        window.requestAnimationFrame(() => nameInput.current?.focus({ preventScroll: true }))
      }
    } finally { finishUpdate() }
  }

  const toggle = async (mod: ModEntry) => {
    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)
    setStatus((current) =>
      current
        ? { ...current, mods: current.mods.map((item) => (item.folder === mod.folder ? { ...item, enabled: !item.enabled } : item)) }
        : current
    )
    try {
      const pendingChanges = await invoke<number>('gimi_toggle_mod', { folder: mod.folder, enabled: !mod.enabled })
      if (mounted.current) setStatus((current) => current ? { ...current, pendingChanges } : current)
    } catch (reason) {
      await alertDialog({ title: '切换模型失败', message: String(reason) })
      await load()
    } finally {
      finishUpdate()
    }
  }

  const toggleEnabled = async () => {
    if (!status || busyRef.current || status.running) return
    busyRef.current = true
    setBusy(true)
    const next = !status.enabled
    setStatus({ ...status, enabled: next })
    try {
      await setConfigOption('gimi_enabled', next)
    } catch (reason) {
      await alertDialog({ title: '保存设置失败', message: String(reason) })
      await load()
    } finally {
      finishUpdate()
    }
  }

  const changeFps = async (fps: number) => {
    if (!status || busyRef.current || status.running) return
    busyRef.current = true
    setBusy(true)
    setStatus((current) => current ? { ...current, fps } : current)
    try { await setConfigOption('gimi_fps', fps) }
    catch (reason) { await alertDialog({ title: '保存帧率设置失败', message: String(reason) }); await load() }
    finally { finishUpdate() }
  }

  const refresh = async (reload = false) => {
    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)
    try { await load(reload ? 'reload' : 'refresh') }
    finally { finishUpdate() }
  }

  const keyword = query.trim().toLowerCase()
  const mods = status ? status.mods.filter((mod) => !keyword || (mod.displayName || mod.name).toLowerCase().includes(keyword) || mod.name.toLowerCase().includes(keyword)) : []
  const enabled = status ? status.mods.filter((mod) => mod.enabled).length : 0

  return (
    <div className="GimiOverlay" hidden={!visible}>
      <div className="GimiPanel" role="dialog" aria-modal="true" aria-label="模型">
        <div className="GimiHead">
          <div>
            <h2 className="GimiTitle">模型</h2>
            <div className="GimiSub">GIMI · MODELS</div>
          </div>
          <button type="button" className="GimiClose" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>

        <div className="GimiMeta">
          <span className={'GimiPill GimiVersion' + (status && status.version ? '' : ' unknown')}>
            {status && status.version ? '原神 ' + status.version : '未识别游戏版本'}
          </span>
          <select
            className="GimiPill GimiFps"
            aria-label="帧率解锁"
            value={status?.fps ?? 0}
            disabled={!status || busy || status.running}
            onChange={(event) => void changeFps(Number(event.target.value))}
          >
            {[0, 30, 60, 90, 120, 144, 240].map((value) => (
              <option key={value} value={value}>
                {value === 0 ? '帧率 默认' : value + ' FPS'}
              </option>
            ))}
          </select>
          <div className="GimiCount">
            已启用 {enabled} / {status ? status.mods.length : 0}
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={!!status?.enabled}
            aria-label="启动游戏时启用模型"
            title={status?.running ? '游戏运行中保持当前状态' : '控制启动游戏时是否加载模型'}
            className={'GimiModelToggle' + (status?.enabled ? ' on' : '')}
            disabled={!status || busy || status.running}
            onClick={() => void toggleEnabled()}
          >
            <span className="GimiModelIndicator" aria-hidden="true" />
            {status?.enabled ? '模型已开启' : '模型已关闭'}
          </button>
        </div>

        {fpsStatus ? <div className="GimiNotice" role="status">{fpsStatus}</div> : null}
        {status && status.pendingChanges > 0 ? <div className="GimiNotice" role="status">{status.pendingChanges} 项选择待重载；启动游戏时会自动应用</div> : null}

        <div className="GimiToolbar">
          <input
            className="GimiSearch"
            placeholder="搜索模型…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <button type="button" className="GimiGhost" disabled={busy} onClick={() => void refresh()}>
            刷新
          </button>
          <button type="button" className="GimiGhost" disabled={!status || busy} onClick={() => void refresh(true)}>
            重载{status && status.pendingChanges > 0 ? ` · ${status.pendingChanges}` : ''}
          </button>
          <button type="button" className="GimiGhost" onClick={() => void invoke('gimi_open_mods').catch(console.error)}>
            打开 Mods 文件夹
          </button>
        </div>

        {!status ? (
          <div className="GimiEmpty">{error || '读取中…'}</div>
        ) : mods.length === 0 ? (
          <div className="GimiEmpty">
            {status.mods.length === 0
              ? 'Mods 文件夹还是空的。把 GIMI 模组目录放进去，再点刷新。'
              : '没有匹配的模型。'}
          </div>
        ) : (
          <div className="GimiList">
            {mods.map((mod) => (
              <div className={'GimiRow' + (mod.enabled ? '' : ' off')} key={mod.folder}>
                <div
                  className="GimiName"
                  title={`双击修改显示名\n原目录：${mod.folder}`}
                  onDoubleClick={() => {
                    if (!busyRef.current && !editing) setEditing({ folder: mod.folder, value: mod.displayName || mod.name, error: '' })
                  }}
                >
                      <input
                        className={'GimiNameField' + (editing?.folder === mod.folder ? ' GimiRenameInput' : '')}
                        ref={editing?.folder === mod.folder ? nameInput : undefined}
                        value={editing?.folder === mod.folder ? editing.value : mod.displayName || mod.name}
                        readOnly={editing?.folder !== mod.folder}
                        tabIndex={editing?.folder === mod.folder ? 0 : -1}
                        aria-label="模组显示名称"
                        aria-invalid={editing?.folder === mod.folder && !!editing.error}
                        maxLength={100}
                        disabled={busy}
                        onChange={(event) => { if (editing?.folder === mod.folder) setEditing({ ...editing, value: event.target.value, error: '' }) }}
                        onBlur={() => { if (editing?.folder === mod.folder) void rename() }}
                        onKeyDown={(event) => {
                          if (editing?.folder !== mod.folder) return
                          if (event.nativeEvent.isComposing || event.keyCode === 229) { event.stopPropagation(); return }
                          if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); void rename() }
                          if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setEditing(null) }
                        }}
                      />
                      {editing?.folder === mod.folder && editing.error ? <div className="GimiRenameError" role="alert">{editing.error}</div> : null}
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={mod.enabled}
                  aria-label={mod.displayName || mod.name}
                  className={'GimiSwitch' + (mod.enabled ? ' on' : '')}
                  disabled={busy}
                  onClick={() => void toggle(mod)}
                >
                  <span className="GimiKnob" />
                </button>
              </div>
            ))}
          </div>
        )}

        <div className="GimiFoot">
          <div className="GimiPath" title={status?.modsPath || ''}>
            {status?.modsPath || ''}
          </div>
          <div className="GimiHint">{busy ? '正在保存或重载模型…' : status?.pendingChanges ? '选好后点重载；忘记操作也会在启动游戏时自动应用' : warming ? '正在后台准备模型配置…' : status?.running ? '同角色模型按 / 切换；重载配置后回游戏按 F10 生效' : '选择后点重载统一应用；同角色模型在游戏内按 / 切换'}</div>
        </div>
      </div>
    </div>
  )
}
