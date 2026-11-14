import React from 'react'
import ReactDOM from 'react-dom/client'

import './index.css'
import App from './ui/App'
import Debug from './ui/Debug'

import { getConfigOption } from './utils/configuration'

const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement)

async function start() {
  const isDebug = await getConfigOption('debug_enabled')
  root.render(<React.StrictMode>{isDebug ? <Debug /> : <App />}</React.StrictMode>)
}
void start().catch((error) => {
  console.error('Unable to initialize application:', error)
  root.render(<div role="alert">无法读取配置：{String(error)}</div>)
})
