import React from 'react'

import Close from '../../../resources/icons/close.svg'

interface IProps {
  value?: string
  initalValue?: string
  placeholder?: string
  onChange?: (value: string) => void
  readOnly?: boolean
  id?: string
  clearable?: boolean
  customClearBehaviour?: () => void
  style?: React.CSSProperties
}

interface IState {
  value: string
}

export default class TextInput extends React.Component<IProps, IState> {
  constructor(props: IProps) {
    super(props)

    this.state = {
      value: props.value ?? props.initalValue ?? '',
    }
  }

  render() {
    return (
      <div className="TextInputWrapper" style={this.props.style || {}}>
        <input
          id={this.props?.id}
          readOnly={this.props.readOnly || false}
          placeholder={this.props.placeholder || ''}
          className="TextInput"
          value={this.props.value ?? this.state.value}
          onChange={(e) => {
            this.setState({ value: e.target.value })
            if (this.props.onChange) this.props.onChange(e.target.value)
          }}
        />
        {this.props.clearable ? (
          <div
            className="TextClear"
            onClick={() => {
              // Run custom behaviour first
              if (this.props.customClearBehaviour) return this.props.customClearBehaviour()

              this.setState({ value: '' })

              if (this.props.onChange) this.props.onChange('')
            }}
          >
            <img src={Close} className="TextInputClear" />
          </div>
        ) : null}
      </div>
    )
  }
}
