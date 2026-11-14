import React from 'react'
import { convertFileSrc, invoke } from '@tauri-apps/api/tauri'
import { wallpaperQueue } from '../../utils/wallpapers'

const ROTATE_MS = 9000
const FADE_MS = 1600
interface IState {
  index: number
  previous: number | null
  started: boolean
  wallpapers: string[]
}
export default class Background extends React.Component<Readonly<unknown>, IState> {
  private timer: ReturnType<typeof setTimeout> | undefined
  private fade: ReturnType<typeof setTimeout> | undefined
  private frame: number | undefined
  private queue: number[] = []
  private loaded = new Map<number, Promise<void>>()
  private live = false
  private revision = 0
  private revealTimer: ReturnType<typeof setTimeout> | undefined
  state: IState = { index: 0, previous: null, started: false, wallpapers: [] }

  componentDidMount() {
    this.live = true
    const revision = ++this.revision
    document.addEventListener('visibilitychange', this.visibility)
    const wallpapers =
      process.env.NODE_ENV === 'development' && !('__TAURI_IPC__' in window)
        ? fetch('/wallpapers-index.json').then((response) => {
            if (!response.ok) throw new Error('Unable to list wallpapers')
            return response.json() as Promise<string[]>
          })
        : invoke<string[]>('list_wallpapers').then((paths) => paths.map((path) => convertFileSrc(path)))
    void wallpapers
      .then((wallpapers) => {
        if (!this.live || revision !== this.revision) return
        const queue = wallpaperQueue(wallpapers.length)
        const first = queue.shift() ?? 0
        this.queue = queue
        // 首张壁纸先以透明状态挂载并等图片解码完成，再触发淡入和 9 秒推镜，
        // 否则图片没加载完动画就已经跑完，看起来就是"首张不放大"
        this.setState({ wallpapers, index: first, started: false }, () => {
          const reveal = () => {
            if (this.live && revision === this.revision) this.reveal()
          }
          this.preload(first).then(reveal, reveal)
        })
      })
      .catch(console.error)
  }
  componentWillUnmount() {
    this.live = false
    this.revision++
    if (this.timer) clearTimeout(this.timer)
    if (this.fade) clearTimeout(this.fade)
    if (this.frame !== undefined) cancelAnimationFrame(this.frame)
    if (this.revealTimer) clearTimeout(this.revealTimer)
    document.removeEventListener('visibilitychange', this.visibility)
    document.body.classList.remove('animations-paused')
  }
  private visibility = () => {
    if (!this.live) return
    // 轮播不再跟随窗口焦点/最小化暂停：只要应用还活着就常驻轮换
    document.body.classList.toggle('animations-paused', document.hidden)
  }
  private next() {
    if (!this.queue.length) {
      this.queue = wallpaperQueue(this.state.wallpapers.length, this.state.index)
    }
    return this.queue[0]
  }
  private preload(index: number) {
    const cached = this.loaded.get(index)
    if (cached) return cached
    const pending = new Promise<void>((resolve, reject) => {
      const image = new Image()
      image.onload = () => resolve()
      image.onerror = () => reject(new Error('Unable to load wallpaper'))
      image.src = this.state.wallpapers[index]
    }).catch((error) => {
      this.loaded.delete(index)
      throw error
    })
    this.loaded.set(index, pending)
    return pending
  }

  private reveal() {
    if (this.frame !== undefined) cancelAnimationFrame(this.frame)
    if (this.revealTimer) clearTimeout(this.revealTimer)
    let applied = false
    const apply = () => {
      if (applied || !this.live) return
      applied = true
      if (this.revealTimer) {
        clearTimeout(this.revealTimer)
        this.revealTimer = undefined
      }
      this.setState({ started: true }, () => this.schedule())
    }
    // 双 rAF 保证未激活状态先真正绘制一帧，transform 的过渡才会从 scale(1) 开始；
    // 窗口最小化时 rAF 会被挂起，加一个定时兜底，恢复窗口时不会闪一帧空白
    this.frame = requestAnimationFrame(() => {
      this.frame = requestAnimationFrame(apply)
    })
    this.revealTimer = setTimeout(apply, 260)
  }
  private schedule() {
    if (!this.live || this.state.wallpapers.length < 2) return
    if (this.timer) clearTimeout(this.timer)
    void this.preload(this.next()).catch(console.error)
    this.timer = setTimeout(() => void this.rotate(), ROTATE_MS)
  }
  private async rotate() {
    const revision = this.revision
    const next = this.next()
    try {
      await this.preload(next)
      if (!this.live || revision !== this.revision) return
      this.queue.shift()
      this.setState((state) => ({ index: next, previous: state.index, started: false }), () => this.reveal())
      if (this.fade) clearTimeout(this.fade)
      this.fade = setTimeout(() => {
        if (this.live) this.setState({ previous: null })
      }, FADE_MS)
    } catch (error) {
      this.queue.shift()
      console.error(error)
    } finally {
      if (revision === this.revision) this.schedule()
    }
  }
  render() {
    const layers = this.state.previous === null ? [this.state.index] : [this.state.previous, this.state.index]
    return (
      <div className="Wallpaper" aria-hidden="true">
        {this.state.wallpapers.length > 0 &&
          layers.map((index) => {
            const previous = index === this.state.previous
            const active = index === this.state.index && this.state.started
            return (
              <div
                key={index}
                className={'WallpaperSlide' + (previous ? ' previous' : active ? ' active' : '')}
                style={{ backgroundImage: `url("${this.state.wallpapers[index]}")` }}
              />
            )
          })}
      </div>
    )
  }
}
