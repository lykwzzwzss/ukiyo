import React from 'react'

// Major Components
import TopBar from './components/TopBar'
import ServerLaunchSection from './components/ServerLaunchSection'
import Options from './components/menu/Options'
import GamePathNotify from './components/menu/GamePathNotify'
import GimiPanel from './components/GimiPanel'
import GimiMark from './components/GimiMark'

import { getConfigOption, setConfigOption } from '../utils/configuration'
import { invoke } from '@tauri-apps/api'
import { listen } from '@tauri-apps/api/event'
import { dataDir } from '@tauri-apps/api/path'
import { appWindow } from '@tauri-apps/api/window'
import Tr from '../utils/language'

// Graphics
import cogBtn from '../resources/icons/cog.svg'
import fushiWordmark from '../resources/fushi-wordmark.svg'

interface IState {
  optionsOpen: boolean
  gimiOpen: boolean
  isGamePathSet: boolean
  panelsReady: boolean
}

export class Main extends React.Component<Readonly<unknown>, IState> {
  constructor(props: Readonly<unknown>) {
    super(props)
    this.state = {
      optionsOpen: false,
      gimiOpen: false,
      isGamePathSet: true,
      panelsReady: false,
    }
  }

  private lifecycle = 0
  private listeners = new Set<() => void>()
  private minimizedForGame = false
  private preloadFrame?: number
  private preloadIdle?: number
  private preloadTimer?: ReturnType<typeof setTimeout>

  componentWillUnmount() {
    this.lifecycle++
    this.listeners.forEach((unlisten) => unlisten())
    this.listeners.clear()
    if (this.preloadFrame !== undefined) window.cancelAnimationFrame(this.preloadFrame)
    if (this.preloadIdle !== undefined) window.cancelIdleCallback(this.preloadIdle)
    if (this.preloadTimer !== undefined) clearTimeout(this.preloadTimer)
  }

  private async updateGameWindow(running: boolean) {
    if (running && !this.minimizedForGame) {
      this.minimizedForGame = true
      try {
        await appWindow.minimize()
      } catch (error) {
        this.minimizedForGame = false
        console.error(error)
      }
    } else if (!running && this.minimizedForGame) {
      this.minimizedForGame = false
      await appWindow.unminimize().catch(console.error)
    }
  }

  async componentDidMount() {
    const lifecycle = ++this.lifecycle
    // Let the main window paint before building the two hidden panels.
    this.preloadFrame = window.requestAnimationFrame(() => {
      const warm = () => {
        if (this.lifecycle === lifecycle && !this.state.panelsReady) this.setState({ panelsReady: true })
      }
      if (window.requestIdleCallback) this.preloadIdle = window.requestIdleCallback(warm, { timeout: 500 })
      else this.preloadTimer = setTimeout(warm, 100)
    })
    let changes = 0
    const subscriptions = await Promise.all([
      listen('lang_error', (event) => console.error(event.payload)),
      listen<boolean>('game_state_changed', (event) => {
        changes++
        if (this.lifecycle === lifecycle) void this.updateGameWindow(event.payload)
      }),
      listen('game_closed', () => {
        changes++
        if (this.lifecycle === lifecycle) void this.updateGameWindow(false)
      }),
    ])
    if (this.lifecycle !== lifecycle) {
      subscriptions.forEach((unlisten) => unlisten())
      return
    }
    subscriptions.forEach((unlisten) => this.listeners.add(unlisten))
    const initialChanges = changes
    const running = await invoke<boolean>('is_game_running')
    if (this.lifecycle !== lifecycle) return
    if (changes === initialChanges) await this.updateGameWindow(running)

    const game_path = await getConfigOption('game_install_path')
    const cert_generated = await getConfigOption('cert_generated')
    const platform = await invoke('get_platform')

    let isGamePathSet = !!game_path

    // 游戏路径失效时清空，提示用户重新选择
    if (game_path && platform === 'windows') {
      const game_exists = (await invoke('dir_exists', { path: game_path })) as boolean
      if (!game_exists) {
        await setConfigOption('game_install_path', '')
        isGamePathSet = false
      }
    }

    if (this.lifecycle !== lifecycle) return
    this.setState({ isGamePathSet })

    if (!cert_generated) {
      // Generate the certificate
      await invoke('generate_ca_files', {
        path: (await dataDir()) + 'cultivation',
      })

      await setConfigOption('cert_generated', true)
    }
  }

  render() {
    return (
      <>
        <TopBar>
          <div
            id="settingsBtn"
            className="TopButton"
            onClick={() => this.setState((state) => ({ optionsOpen: !state.optionsOpen, gimiOpen: false, panelsReady: true }))}
          >
            <img src={cogBtn} alt="settings" />
          </div>
          <div
            id="gimiBtn"
            className="TopButton"
            title="模型"
            onClick={() => this.setState((state) => ({ gimiOpen: !state.gimiOpen, optionsOpen: false, panelsReady: true }))}
          >
            <GimiMark />
          </div>
        </TopBar>

        {this.state.isGamePathSet ? null : <GamePathNotify />}

        <section className="HeroLayer">
          <div className="Hero">
            <h1 className="HeroTitle" aria-label="浮世">
              <img className="HeroWordmark" src={fushiWordmark} alt="" aria-hidden="true" draggable={false} />
            </h1>
            <div className="HeroRule">
              <span className="HeroRuleLine" />
              <span className="HeroRuleDiamond" />
              <span className="HeroRuleLine" />
            </div>
            <p className="HeroSubtitle">
              <Tr text="main.hero_subtitle" />
            </p>
            <div className="HeroChips">
              <span className="StatusChip">
                <i className="StatusChipDot" />
                <Tr text="main.chip_gc" />
              </span>
              <span className="StatusChip">
                <i className="StatusChipDot" />
                <Tr text="main.chip_proxy" />
              </span>
            </div>
          </div>
        </section>

        <ServerLaunchSection />

        {this.state.panelsReady ? <GimiPanel visible={this.state.gimiOpen} onClose={() => this.setState({ gimiOpen: false })} /> : null}

        {this.state.panelsReady ? (
          <Options
            visible={this.state.optionsOpen}
            closeFn={() => {
              this.setState({ optionsOpen: false })
            }}
          />
        ) : null}
      </>
    )
  }
}
