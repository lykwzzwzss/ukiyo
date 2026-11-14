import React, { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import './Dialog.css'

export interface DialogOptions {
  title?: string
  message: string
  confirmText?: string
  cancelText?: string
  cancelable?: boolean
}

interface DialogRequest extends DialogOptions {
  resolve: (value: boolean) => void
}

let current: DialogRequest | null = null
const queue: DialogRequest[] = []
let notify: ((request: DialogRequest | null) => void) | null = null

function advance() {
  if (!current && queue.length > 0) current = queue.shift() ?? null
  if (notify) notify(current)
}

function open(options: DialogOptions): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    queue.push({ ...options, resolve })
    advance()
  })
}

export function alertDialog(options: Omit<DialogOptions, 'cancelable'>): Promise<boolean> {
  return open({ ...options, cancelable: false })
}

export function confirmDialog(options: Omit<DialogOptions, 'cancelable'>): Promise<boolean> {
  return open({ ...options, cancelable: true })
}

function settle(value: boolean) {
  const request = current
  current = null
  if (request) request.resolve(value)
  advance()
}

export function DialogHost() {
  const [request, setRequest] = useState<DialogRequest | null>(null)

  useEffect(() => {
    notify = setRequest
    advance()
    return () => {
      if (notify === setRequest) notify = null
    }
  }, [])

  useEffect(() => {
    if (!request) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        settle(false)
      } else if (event.key === 'Enter') {
        event.preventDefault()
        settle(true)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [request])

  if (!request) return null

  const cancelable = request.cancelable !== false
  const confirm = () => settle(true)
  const cancel = () => settle(false)

  return createPortal(
    <div
      className="DialogOverlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && cancelable) cancel()
      }}
    >
      <div className="Dialog" role="dialog" aria-modal="true" aria-label={request.title || '提示'}>
        {request.title ? <h2 className="DialogTitle">{request.title}</h2> : null}
        <p className="DialogMessage">{request.message}</p>
        <div className="DialogActions">
          {cancelable ? (
            <button type="button" className="DialogButton" onClick={cancel} autoFocus>
              {request.cancelText || '取消'}
            </button>
          ) : null}
          <button type="button" className="DialogButton primary" onClick={confirm} autoFocus={!cancelable}>
            {request.confirmText || '确定'}
          </button>
        </div>
      </div>
    </div>,
    document.body
  )
}
