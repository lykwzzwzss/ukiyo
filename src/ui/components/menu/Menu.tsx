import React from 'react'

import Close from '../../../resources/icons/close.svg'

interface IProps {
  children: React.ReactNode[] | React.ReactNode
  className?: string
  heading: string
  closeFn: () => void
  hidden?: boolean
}

export default class Menu extends React.Component<IProps, never> {
  constructor(props: IProps) {
    super(props)
  }

  render() {
    return (
      <div className={'Menu ' + this.props.className} id="menuContainer" hidden={this.props.hidden}>
        <div className="MenuTop" id="menuContainerTop">
          <div className="MenuHeading" id="menuHeading">
            {this.props.heading}
          </div>
          <div className="MenuExit" id="menuButtonCloseContainer" onClick={this.props.closeFn}>
            <img src={Close} className="MenuClose" id="menuButtonCloseIcon" />
          </div>
        </div>
        <div className="MenuInner" id="menuContent">
          {this.props.children}
        </div>
      </div>
    )
  }
}
