import React from 'react'
import './App.css'

import { Main } from './Main'
import Background from './components/Background'
import Ambient from './components/Ambient'
import { DialogHost } from './components/common/Dialog'

import './theme.css'

export default class App extends React.Component {
  render() {
    return (
      <div className="App">
        <Background />
        <div className="AppVeil" />
        <div className="AppGlow" />

        <Ambient />

        <Main />
        <DialogHost />
      </div>
    )
  }
}
