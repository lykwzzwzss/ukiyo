import React from 'react'
import Menu from './Menu'
import DirInput from '../common/DirInput'
import Checkbox from '../common/Checkbox'
import TextInput from '../common/TextInput'
import BigButton from '../common/BigButton'
import PopupSelect from '../common/PopupSelect'

import { Configuration, getConfig, getConfigOption, setConfigOption, subscribeConfig } from '../../../utils/configuration'
import Tr, { getLanguages, translate } from '../../../utils/language'
import { unpatchGame } from '../../../utils/rsa'


interface IProps {
  closeFn: () => void
  visible: boolean
}

interface ILanguage {
  code: string
  name: string
}

interface IState {
  heading: string
  game_install_path: string
  patch_rsa: boolean
  use_internal_proxy: boolean
  wipe_login: boolean
  un_elevated: boolean
  show_version: boolean
  launch_args: string
  languages: ILanguage[]
  language: string
}

export default class Options extends React.Component<IProps, IState> {
  constructor(props: IProps) {
    super(props)

    this.state = {
      heading: 'Settings',
      game_install_path: '',
      patch_rsa: true,
      use_internal_proxy: true,
      wipe_login: false,
      un_elevated: false,
      show_version: true,
      launch_args: '',
      languages: [],
      language: 'en',
    }

    this.setGameExecutable = this.setGameExecutable.bind(this)
    this.toggleOption = this.toggleOption.bind(this)
    this.setLaunchArgs = this.setLaunchArgs.bind(this)
    this.setLanguage = this.setLanguage.bind(this)
    this.removeRSA = this.removeRSA.bind(this)
  }

  private alive = false
  private unsubscribe?: () => void

  private applyConfig = (config: Configuration) => {
    if (!this.alive) return
    this.setState({
      game_install_path: config.game_install_path || '',
      patch_rsa: config.patch_rsa,
      use_internal_proxy: config.use_internal_proxy,
      wipe_login: config.wipe_login,
      un_elevated: config.un_elevated,
      show_version: config.show_version,
      launch_args: this.pendingArgs ?? config.launch_args ?? '',
      language: config.language || 'en',
    })
  }

  async componentDidMount() {
    this.alive = true
    this.unsubscribe = subscribeConfig(this.applyConfig)
    try {
      const [choices, heading] = await Promise.all([getLanguages(), translate('options.title')])
      const config = await getConfig()
      if (!this.alive) return
      this.setState({ heading, languages: choices.map((item) => {
        const code = Object.keys(item)[0]
        return { code, name: item[code] }
      }) })
      this.applyConfig(config)
    } catch (error) { console.error(error) }
  }

  componentDidUpdate(previous: IProps) {
    if (previous.visible && !this.props.visible) this.flushArgs()
  }

  async setGameExecutable(path: string) {
    await setConfigOption('game_install_path', path)
    this.setState({ game_install_path: path })
  }

  async toggleOption(opt: 'patch_rsa' | 'use_internal_proxy' | 'wipe_login' | 'un_elevated' | 'show_version') {
    const changedVal = !(await getConfigOption(opt))
    await setConfigOption(opt, changedVal)

    this.setState({ [opt]: changedVal } as unknown as IState)
  }

  private saveArgsTimer: ReturnType<typeof setTimeout> | undefined
  private pendingArgs: string | undefined

  componentWillUnmount() {
    this.alive = false
    this.unsubscribe?.()
    this.flushArgs()
  }

  private flushArgs() {
    if (this.saveArgsTimer) clearTimeout(this.saveArgsTimer)
    if (this.pendingArgs !== undefined) {
      const args = this.pendingArgs
      this.pendingArgs = undefined
      void setConfigOption('launch_args', args).catch(console.error)
    }
  }

  setLaunchArgs(value: string) {
    this.setState({ launch_args: value })
    this.pendingArgs = value
    if (this.saveArgsTimer) clearTimeout(this.saveArgsTimer)
    this.saveArgsTimer = setTimeout(() => {
      this.pendingArgs = undefined
      void setConfigOption('launch_args', value).catch(console.error)
    }, 300)
  }

  async setLanguage(value: string) {
    if (this.saveArgsTimer) clearTimeout(this.saveArgsTimer)
    if (this.pendingArgs !== undefined) {
      const args = this.pendingArgs
      this.pendingArgs = undefined
      await setConfigOption('launch_args', args)
    }
    await setConfigOption('language', value)
    window.location.reload()
  }

  async removeRSA() {
    await unpatchGame()
  }

  render() {
    return (
      <Menu closeFn={this.props.closeFn} className="Options" heading={this.state.heading} hidden={!this.props.visible}>
        <div className="OptionSection" id="menuOptionsContainerGamePath">
          <div className="OptionLabel">
            <Tr text="options.game_path" />
          </div>
          <div className="OptionValue">
            <DirInput
              onChange={this.setGameExecutable}
              value={this.state.game_install_path}
              extensions={['exe']}
              clearable={false}
            />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.patch_rsa" />
          </div>
          <div className="OptionValue">
            <Checkbox onChange={() => this.toggleOption('patch_rsa')} checked={this.state.patch_rsa} id="patchMeta" />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.use_proxy" />
          </div>
          <div className="OptionValue">
            <Checkbox
              onChange={() => this.toggleOption('use_internal_proxy')}
              checked={this.state.use_internal_proxy}
              id="useProxy"
            />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.wipe_login" />
          </div>
          <div className="OptionValue">
            <Checkbox onChange={() => this.toggleOption('wipe_login')} checked={this.state.wipe_login} id="wipeLogin" />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.un_elevated" />
          </div>
          <div className="OptionValue">
            <Checkbox
              onChange={() => this.toggleOption('un_elevated')}
              checked={this.state.un_elevated}
              id="unElevated"
            />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.show_version" />
          </div>
          <div className="OptionValue">
            <Checkbox
              onChange={() => this.toggleOption('show_version')}
              checked={this.state.show_version}
              id="showVersion"
            />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.launch_args" />
          </div>
          <div className="OptionValue">
            <TextInput
              id="launchArgs"
              placeholder={'-arg=value'}
              value={this.state.launch_args}
              onChange={this.setLaunchArgs}
              clearable={true}
            />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel" id="languageLabel">
            <Tr text="options.language" />
          </div>
          <div className="OptionValue">
            <PopupSelect
              visible={this.props.visible}
              id="languageSelect"
              value={this.state.language}
              labelledBy="languageLabel"
              options={this.state.languages.map((language) => ({ value: language.code, label: language.name }))}
              onChange={(value) => {
                void this.setLanguage(value).catch(console.error)
              }}
            />
          </div>
        </div>

        <div className="OptionSection">
          <div className="OptionLabel">
            <Tr text="options.recover_rsa" />
          </div>
          <div className="OptionValue">
            <BigButton onClick={this.removeRSA} id="removeRSA">
              <Tr text="components.delete" />
            </BigButton>
          </div>
        </div>
      </Menu>
    )
  }
}
