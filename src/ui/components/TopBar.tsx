import React from 'react'
import { app, invoke } from '@tauri-apps/api'
import { appWindow } from '@tauri-apps/api/window'
import { confirmDialog } from './common/Dialog'

import closeIcon from '../../resources/icons/close.svg'
import minIcon from '../../resources/icons/min.svg'

interface IProps {
  children?: React.ReactNode | React.ReactNode[]
}

interface IState {
  version: string
}

export default class TopBar extends React.Component<IProps, IState> {
  constructor(props: IProps) {
    super(props)

    this.state = {
      version: '0.0.0',
    }
  }

  async componentDidMount() {
    const version = await app.getVersion()
    this.setState({ version })
  }

  async handleClose() {
    if (await invoke('is_game_running')) {
      const confirmed = await confirmDialog({
        title: '游戏正在运行',
        message: '现在退出不会卸载补丁，启动器也会断开当前连接。确定要退出吗？',
        confirmText: '仍然退出',
        cancelText: '取消',
      })
      if (!confirmed) {
        return
      }
    }

    await invoke('disconnect')
    await appWindow.close()
  }

  handleMinimize() {
    appWindow.minimize()
  }

  render() {
    return (
      <div className="TopBar" id="topBarContainer" data-tauri-drag-region>
        <div id="title" data-tauri-drag-region>
          <span className="BrandMark" data-tauri-drag-region />
          <span className="BrandName" data-tauri-drag-region>
            浮世
          </span>
          <span id="version" data-tauri-drag-region>
            v{this.state?.version}
          </span>
        </div>

        <div className="TopBtns" id="topBarButtonContainer">
          <div id="closeBtn" onClick={this.handleClose} className="TopButton">
            <img src={closeIcon} alt="close" />
          </div>
          <div id="minBtn" onClick={this.handleMinimize} className="TopButton">
            <img src={minIcon} alt="minimize" />
          </div>
          {this.props.children}
        </div>
      </div>
    )
  }
}
