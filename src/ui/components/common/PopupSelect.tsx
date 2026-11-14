import React from 'react'
import { createPortal } from 'react-dom'
import './PopupSelect.css'

interface Choice {
  value: string
  label: string
}
interface Props {
  id: string
  value: string
  options: Choice[]
  labelledBy: string
  onChange: (value: string) => void
  visible?: boolean
}
interface Placement {
  left: number
  top: number
  width: number
  maxHeight: number
}
interface State {
  open: boolean
  active: number
  placement: Placement
}

export function dropdownPlacement(
  anchor: { left: number; top: number; bottom: number; width: number },
  viewport: { width: number; height: number },
  count: number
): Placement {
  const gap = 6
  const edge = 8
  const desired = Math.min(254, count * 36 + 14)
  const above = Math.max(0, anchor.top - gap - edge)
  const below = Math.max(0, viewport.height - anchor.bottom - gap - edge)
  const upwards = below < desired && above > below
  const maxHeight = Math.min(desired, upwards ? above : below)
  const width = Math.min(anchor.width, Math.max(0, viewport.width - edge * 2))
  return {
    left: Math.max(edge, Math.min(anchor.left, viewport.width - width - edge)),
    top: upwards ? anchor.top - gap - maxHeight : anchor.bottom + gap,
    width,
    maxHeight,
  }
}

export default class PopupSelect extends React.Component<Props, State> {
  state: State = { open: false, active: 0, placement: { left: 0, top: 0, width: 0, maxHeight: 0 } }
  private trigger = React.createRef<HTMLButtonElement>()
  private list = React.createRef<HTMLDivElement>()

  private attach() {
    document.addEventListener('pointerdown', this.outside, true)
    document.addEventListener('scroll', this.onScroll, true)
    document.addEventListener('animationend', this.reposition, true)
    window.addEventListener('resize', this.reposition)
  }
  private detach() {
    document.removeEventListener('pointerdown', this.outside, true)
    document.removeEventListener('scroll', this.onScroll, true)
    document.removeEventListener('animationend', this.reposition, true)
    window.removeEventListener('resize', this.reposition)
  }
  componentDidUpdate(previous: Props, state: State) {
    const wasOpen = previous.visible !== false && state.open
    const isOpen = this.props.visible !== false && this.state.open
    if (!wasOpen && isOpen) this.attach()
    if (wasOpen && !isOpen) this.detach()
    if (this.props.visible === false && this.state.open) this.close(false)
  }
  componentWillUnmount() { this.detach() }
  private outside = (event: PointerEvent) => {
    if (
      event.target instanceof Node &&
      !this.trigger.current?.contains(event.target) &&
      !this.list.current?.contains(event.target)
    ) {
      this.close(false)
    }
  }
  private onScroll = (event: Event) => {
    if (event.target instanceof Node && !this.list.current?.contains(event.target)) this.close(false)
  }
  private placement() {
    const anchor = this.trigger.current?.getBoundingClientRect()
    return anchor
      ? dropdownPlacement(anchor, { width: window.innerWidth, height: window.innerHeight }, this.props.options.length)
      : this.state.placement
  }
  private reposition = () => {
    if (!this.state.open) return
    const placement = this.placement()
    this.setState((state) =>
      Object.keys(placement).some(
        (key) => placement[key as keyof Placement] !== state.placement[key as keyof Placement]
      )
        ? { placement }
        : null
    )
  }
  private reveal = () => {
    this.list.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' })
  }
  private open = () => {
    if (this.props.visible === false || !this.props.options.length) return
    const active = Math.max(
      0,
      this.props.options.findIndex((option) => option.value === this.props.value)
    )
    this.setState({ open: true, active, placement: this.placement() }, () => {
      this.list.current?.focus()
      this.reveal()
    })
  }
  private close(focus = true) {
    if (!this.state.open) return
    this.setState({ open: false }, () => {
      if (focus) this.trigger.current?.focus()
    })
  }
  private choose = (index: number) => {
    const option = this.props.options[index]
    if (!option) return
    this.close()
    if (option.value !== this.props.value) this.props.onChange(option.value)
  }
  private keyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      this.close()
      return
    }
    if (event.key === 'Tab') {
      this.close(false)
      this.trigger.current?.focus()
      return
    }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      this.choose(this.state.active)
      return
    }
    const maximum = this.props.options.length - 1
    let active = this.state.active
    if (event.key === 'ArrowDown') active = Math.min(maximum, active + 1)
    else if (event.key === 'ArrowUp') active = Math.max(0, active - 1)
    else if (event.key === 'Home') active = 0
    else if (event.key === 'End') active = maximum
    else return
    event.preventDefault()
    this.setState({ active }, this.reveal)
  }
  render() {
    const selected = this.props.options.find((option) => option.value === this.props.value)
    const listId = this.props.id + '-list'
    return (
      <>
        <button
          type="button"
          id={this.props.id}
          className="PopupSelect"
          ref={this.trigger}
          aria-labelledby={this.props.labelledBy + ' ' + this.props.id + '-value'}
          aria-haspopup="listbox"
          aria-expanded={this.state.open}
          aria-controls={this.state.open ? listId : undefined}
          disabled={!this.props.options.length}
          onClick={() => (this.state.open ? this.close() : this.open())}
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
              event.preventDefault()
              this.open()
            }
          }}
        >
          <span id={this.props.id + '-value'}>{selected?.label || this.props.value}</span>
          <span className="PopupSelectArrowBox" aria-hidden="true">
            <span className="PopupSelectArrow" />
          </span>
        </button>
        {this.props.visible !== false && this.state.open &&
          createPortal(
            <div
              id={listId}
              ref={this.list}
              className="PopupSelectList"
              style={this.state.placement}
              role="listbox"
              tabIndex={-1}
              aria-labelledby={this.props.labelledBy}
              aria-activedescendant={listId + '-' + this.state.active}
              onKeyDown={this.keyDown}
            >
              {this.props.options.map((option, index) => (
                <div
                  key={option.value}
                  id={listId + '-' + index}
                  role="option"
                  aria-selected={option.value === this.props.value}
                  data-active={index === this.state.active}
                  className="PopupSelectOption"
                  onPointerMove={() => {
                    if (index !== this.state.active) this.setState({ active: index })
                  }}
                  onClick={() => this.choose(index)}
                >
                  <span>{option.label}</span>
                  <span aria-hidden="true">{option.value === this.props.value ? '✓' : ''}</span>
                </div>
              ))}
            </div>,
            document.body
          )}
      </>
    )
  }
}
