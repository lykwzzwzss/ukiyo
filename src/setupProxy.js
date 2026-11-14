// Serve external wallpapers during browser-only frontend development.
// Production uses Tauri's scoped asset protocol and does not embed these files.
const express = require('express')
const path = require('path')
const fs = require('fs')

module.exports = function (app) {
  const directory = path.join(__dirname, '../src-tauri/assets/wallpapers')
  app.get('/wallpapers-index.json', async (_request, response, next) => {
    try {
      const entries = await fs.promises.readdir(directory, { withFileTypes: true })
      response.json(
        entries
          .filter((entry) => entry.isFile() && /\.(jpe?g|jfif|png|webp|gif|bmp|avif)$/i.test(entry.name))
          .map((entry) => '/wallpapers/' + encodeURIComponent(entry.name))
          .sort()
      )
    } catch (error) {
      if (error.code === 'ENOENT') response.json([])
      else next(error)
    }
  })
  app.use('/wallpapers', express.static(directory))
}
