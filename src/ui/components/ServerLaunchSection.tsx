import React from 'react'
import BigButton from './common/BigButton'
import TextInput from './common/TextInput'
import { getConfig, updateConfig, subscribeConfig } from '../../utils/configuration'
import Tr, { translate } from '../../utils/language'
import { invoke } from '@tauri-apps/api/tauri'
import { dataDir } from '@tauri-apps/api/path'
import { getGameExecutable, getGameVersion } from '../../utils/game'
import { patchGame } from '../../utils/rsa'
import { alertDialog } from './common/Dialog'


interface IState {
  launching: boolean
  buttonLabel: string
  ip: string
  port: string
  ipPlaceholder: string
  portPlaceholder: string
}

export default class ServerLaunchSection extends React.Component<Readonly<unknown>, IState> {
  constructor(props: Readonly<unknown>) {
    super(props)

    this.state = {
      launching: false,
      buttonLabel: '',
      ip: '',
      port: '',
      ipPlaceholder: '',
      portPlaceholder: '',
    }

    this.playGame = this.playGame.bind(this)
    this.setIp = this.setIp.bind(this)
    this.setPort = this.setPort.bind(this)
    this.setButtonLabel = this.setButtonLabel.bind(this)
  }

  private mountedGeneration = 0
  private labelRequest = 0
  private unsubscribeConfig: (() => void) | undefined

  async componentDidMount() {
    const generation = ++this.mountedGeneration
    this.unsubscribeConfig = subscribeConfig((config, previous) => {
      if (
        !previous ||
        config.game_install_path !== previous.game_install_path ||
        config.show_version !== previous.show_version ||
        config.language !== previous.language
      ) {
        this.setState({ buttonLabel: '' })
        void this.setButtonLabel()
      }
    })
    const config = await getConfig()
    const [ipPlaceholder, portPlaceholder] = await Promise.all([
      translate('main.ip_placeholder'),
      translate('help.port_placeholder'),
    ])
    if (generation !== this.mountedGeneration) return
    this.setState({
      ip: config.last_ip || 'localhost',
      port: config.last_port || '443',
      ipPlaceholder,
      portPlaceholder,
    })
    void this.setButtonLabel()
  }

  componentWillUnmount() {
    this.mountedGeneration++
    this.labelRequest++
    this.unsubscribeConfig?.()
  }

  private launchInFlight = false

  async playGame() {
    if (this.launchInFlight) return
    this.launchInFlight = true
    this.setState({ launching: true })
    try {
      if (await invoke<boolean>('is_game_running')) return
      await this.launchGame()
    } catch (error) {
      if (!(await invoke<boolean>('is_game_running'))) {
        await Promise.allSettled([invoke('disconnect'), invoke('unpatch_game')])
      }
      console.error('Game launch failed:', error)
      await alertDialog({
        title: '启动失败',
        message: '启动游戏失败：' + String(error),
      })
    } finally {
      this.launchInFlight = false
      this.setState({ launching: false })
    }
  }

  async launchGame() {
    const config = await getConfig()

    if (!(await getGameExecutable())) {
      await alertDialog({
        title: '未设置游戏路径',
        message: '请先在设置中选择游戏安装路径。',
      })
      return
    }

    if (
      !this.state.ip.trim() ||
      !/^\d+$/.test(this.state.port) ||
      Number(this.state.port) < 1 ||
      Number(this.state.port) > 65535
    ) {
      await alertDialog({
        title: '端口无效',
        message: '请填写服务器地址，以及 1–65535 之间的端口。',
      })
      return
    }

    const game_exe = await getGameExecutable()
    const patchable = game_exe?.toLowerCase().includes('yuanshen') || game_exe?.toLowerCase().includes('genshin')

    const gameExists = await invoke('dir_exists', {
      path: config.game_install_path,
    })

    if (!gameExists) {
      throw new Error('未找到游戏！路径：' + config.game_install_path)
    }

    if (config.patch_rsa && patchable) {
      const gameVersion = await getGameVersion()

      if (gameVersion == null) {
        await alertDialog({
          title: '无法识别版本',
          message: '无法识别游戏版本，请确认游戏路径是否正确。',
        })
        return
      }

      if (gameVersion?.major == 2 && gameVersion?.minor < 9) {
        await alertDialog({
          title: '游戏版本过旧',
          message: '该版本无法使用 RSA 补丁，请先在设置中关闭 RSA 补丁。',
        })
        return
      }

      if (gameVersion?.major == 3 && gameVersion?.minor < 1) {
        await alertDialog({
          title: '游戏版本过旧',
          message: '该版本无法使用 RSA 补丁，请先在设置中关闭 RSA 补丁。',
        })
        return
      }

      if (gameVersion?.major == 4 && gameVersion?.minor == 5) {
        await alertDialog({
          title: '版本不兼容',
          message: '游戏 4.5 请使用 Cultivation 1.4.0 版本启动。',
          confirmText: '知道了',
        })
        return
      }

      const versionString = `${gameVersion.major}.${gameVersion.minor}`
      const patched = await patchGame(false, versionString, 'http://' + this.state.ip.trim() + ':' + this.state.port)

      if (!patched) {
        await alertDialog({
          title: '补丁失败',
          message:
            '当前游戏版本没有对应的补丁文件。' +
            '\n请确认游戏版本，并确保 浮世安装目录下有对应版本的补丁。' +
            '\n\n当前游戏版本：' +
            gameVersion?.major +
            '.' +
            gameVersion?.minor,
        })
        return
      }
    }

    await updateConfig({ last_ip: this.state.ip, last_port: this.state.port })

    if (config.use_internal_proxy) {
      await invoke('set_proxy_addr', {
        addr: 'http://' + this.state.ip + ':' + this.state.port,
      })
      await invoke('connect', {
        port: 8365,
        certificatePath: (await dataDir()) + '\\cultivation\\ca',
      })
    }

    if (config.wipe_login) {
      await invoke('wipe_registry', {
        execName: (await getGameExecutable())?.split('.exe')[0].replace(/([a-z\d])([A-Z])/g, '$1 $2'),
      })
    }

    await invoke('run_program', {
      path: config.game_install_path,
      args: config.launch_args,
      runAsUser: config.un_elevated,
    })
  }

  setIp(text: string) {
    this.setState({ ip: text })
  }

  setPort(text: string) {
    this.setState({ port: text })
  }

  async setButtonLabel() {
    const request = ++this.labelRequest
    const generation = this.mountedGeneration
    const config = await getConfig()
    const [label, version] = await Promise.all([
      translate('main.launch_button'),
      config.show_version ? getGameVersion(config.game_install_path) : Promise.resolve(null),
    ])
    if (request !== this.labelRequest || generation !== this.mountedGeneration) return
    this.setState({ buttonLabel: version ? `${label}  ${version.major}.${version.minor}` : label })
  }

  render() {
    return (
      <div className="ConnectPanel" id="connectPanel">
        <div className="ConnectHeader">
          <div className="ConnectTitle">
            <Tr text="main.connect_server" />
          </div>
          <div className="ConnectSub">SERVER CONNECTION</div>
        </div>

        <div className="ConnectRule" />

        <div className="ServerConfig" id="serverConfigContainer">
          <label className="FieldLabel" htmlFor="ip">
            <Tr text="main.server_address" />
          </label>
          <TextInput
            id="ip"
            key="ip"
            placeholder={this.state.ipPlaceholder}
            onChange={this.setIp}
            value={this.state.ip}
          />

          <label className="FieldLabel" htmlFor="port">
            <Tr text="main.port" />
          </label>
          <TextInput
            id="port"
            key="port"
            placeholder={this.state.portPlaceholder}
            onChange={this.setPort}
            value={this.state.port}
          />
        </div>

        <div className="ConnectStatus">
          <span className="StatusDot" />
          <Tr text="main.status_enabled" />
        </div>

        <BigButton onClick={this.playGame} id="launchGame" disabled={this.state.launching}>
          {this.state.buttonLabel || <Tr text="main.launch_button" />}
        </BigButton>
      </div>
    )
  }
}
