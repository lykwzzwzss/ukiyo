const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const path = require('node:path')
const project = path.resolve(__dirname, '..')
const ts = require(path.join(project, 'node_modules/typescript'))
const React = require(path.join(project, 'node_modules/react'))
function moduleAt(file, requireMock) {
  const sandbox = { exports: {}, console, setTimeout, clearTimeout, require: requireMock }
  const source = fs.readFileSync(path.join(project, file), 'utf8')
  vm.runInNewContext(
    ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.React, esModuleInterop: true },
    }).outputText,
    sandbox
  )
  return sandbox.exports
}
function configuration(raw) {
  const files = new Map(raw === undefined ? [] : [['mock/cultivation/configuration.json', raw]])
  const stats = { reads: 0, writes: 0, directories: 0, failRename: false }
  const mockFs = {
    createDir: async () => {
      stats.directories++
    },
    readTextFile: async (path) => {
      stats.reads++
      if (!files.has(path)) throw Error('Missing file')
      return files.get(path)
    },
    writeFile: async (file) => {
      stats.writes++
      files.set(file.path, file.contents)
    },
    renameFile: async (from, to) => {
      if (stats.failRename) {
        stats.failRename = false
        throw Error('Rename failed')
      }
      files.set(to, files.get(from))
      files.delete(from)
    },
    copyFile: async (from, to) => {
      files.set(to, files.get(from))
    },
  }
  const api = {
    fs: mockFs,
    invoke: async (command, args) => {
      assert.equal(command, 'does_file_exist')
      return files.has(args.path1)
    },
  }
  const config = moduleAt('src/utils/configuration.ts', (name) =>
    name === '@tauri-apps/api' || name === '@tauri-apps/api/tauri' ? api : { dataDir: async () => 'mock/' }
  )
  return { config, files, stats }
}
;(async () => {
  const { config, files, stats } = configuration(JSON.stringify({ profile: 'default', language: 'en' }))
  await Promise.all([config.setConfigOption('language', 'chs'), config.setConfigOption('patch_rsa', false)])
  assert.equal((await config.getConfig()).language, 'chs')
  assert.equal((await config.getConfig()).patch_rsa, false)
  assert.equal((await config.getConfig()).launch_args, '')
  const reads = stats.reads,
    directories = stats.directories
  await Promise.all(Array.from({ length: 30 }, () => config.getConfigOption('language')))
  assert.equal(stats.reads, reads)
  assert.equal(stats.directories, directories)
  const snapshot = await config.getConfig()
  snapshot.language = 'external'
  assert.equal((await config.getConfig()).language, 'chs')
  const before = files.get('mock/cultivation/configuration.json')
  stats.failRename = true
  await assert.rejects(config.setConfigOption('language', 'failed'))
  assert.equal(files.get('mock/cultivation/configuration.json'), before)
  await config.setConfigOption('show_version', false)
  assert.equal((await config.getConfig()).language, 'chs')
  const corrupt = configuration('{broken')
  assert.equal((await corrupt.config.getConfig()).launch_args, '')
  assert([...corrupt.files.keys()].some((key) => key.includes('.corrupt-')))
  const created = configuration()
  assert.equal((await created.config.getConfig()).game_install_path, '')
  await created.config.setConfigOption('last_ip', 'localhost')
  assert(created.files.has('mock/cultivation/configuration.json'))
  await config.setConfigOption('gimi_fps', -1)
  assert.equal((await config.getConfig()).gimi_fps, 0)
  await config.setConfigOption('gimi_fps', 144)
  assert.equal((await config.getConfig()).gimi_fps, 144)
  await config.setConfigOption('gimi_fps', 12.5)
  assert.equal((await config.getConfig()).gimi_fps, 0)
  const languageCalls = new Map()
  const lang = moduleAt('src/utils/language.ts', (name) => {
    if (name === 'react') return React
    if (name === './configuration') return { getConfigOption: async () => 'chs' }
    return {
      invoke: async (_, args) => {
        languageCalls.set(args.lang, (languageCalls.get(args.lang) || 0) + 1)
        return JSON.stringify(args.lang === 'chs' ? { main: { title: '测试' } } : { main: { fallback: 'Fallback' } })
      },
    }
  })
  const translations = await Promise.all(Array.from({ length: 30 }, () => lang.translate('main.title')))
  assert(translations.every((value) => value === '测试'))
  assert.equal(await lang.translate('main.fallback'), 'Fallback')
  assert.equal(languageCalls.get('chs'), 1)
  assert.equal(languageCalls.get('en'), 1)
  const Input = moduleAt('src/ui/components/common/TextInput.tsx', (name) => (name === 'react' ? React : {})).default
  const input = new Input({ value: 'old' })
  input.props = { value: '' }
  assert.equal(input.render().props.children[0].props.value, '')

  const notifications = []
  const unsubscribe = config.subscribeConfig((next, previous) =>
    notifications.push([next.game_install_path, previous.game_install_path])
  )
  await config.setConfigOption('game_install_path', 'E:/YuanShen_3.4.0/YuanShen.exe')
  assert.equal(notifications.length, 1)
  assert.equal(notifications[0][0], 'E:/YuanShen_3.4.0/YuanShen.exe')
  unsubscribe()
  await config.setConfigOption('game_install_path', 'another-path')
  assert.equal(notifications.length, 1)

  const popupModule = moduleAt('src/ui/components/common/PopupSelect.tsx', (name) => (name === 'react' ? React : {}))
  const place = popupModule.dropdownPlacement
  const lower = place({ left: 400, top: 610, bottom: 652, width: 300 }, { width: 960, height: 720 }, 16)
  assert(lower.top + lower.maxHeight <= 604)
  assert.equal(lower.width, 300)
  const upper = place({ left: 40, top: 20, bottom: 62, width: 300 }, { width: 960, height: 720 }, 16)
  assert.equal(upper.top, 68)
  const narrow = place({ left: 180, top: 200, bottom: 242, width: 300 }, { width: 240, height: 300 }, 16)
  assert(narrow.left >= 8 && narrow.left + narrow.width <= 232)
  assert(narrow.top >= 8 && narrow.top + narrow.maxHeight <= 292)

  let selection = {
    game_install_path: 'game-4.7',
    show_version: true,
    language: 'chs',
    use_internal_proxy: true,
    https_enabled: false,
    last_ip: 'localhost',
    last_port: '443',
  }
  let changed
  let unsubscribed = false
  const pendingVersions = new Map()
  const Server = moduleAt('src/ui/components/ServerLaunchSection.tsx', (name) => {
    if (name === 'react') return React
    if (name === '../../utils/configuration')
      return {
        getConfig: async () => ({ ...selection }),
        updateConfig: async () => {},
        subscribeConfig: (listener) => {
          changed = listener
          return () => {
            unsubscribed = true
          }
        },
      }
    if (name === '../../utils/language')
      return { translate: async (key) => (key === 'main.launch_button' ? '启动游戏' : key) }
    if (name === '../../utils/game')
      return { getGameVersion: (path) => new Promise((resolve) => pendingVersions.set(path, resolve)) }
    return {}
  }).default
  const section = new Server({})
  section.setState = (update, callback) => {
    Object.assign(section.state, typeof update === 'function' ? update(section.state, section.props) : update)
    callback?.()
  }
  await section.componentDidMount()
  await new Promise((resolve) => setImmediate(resolve))
  assert(pendingVersions.has('game-4.7'))
  const previous = { ...selection }
  selection.game_install_path = 'game-3.4'
  changed({ ...selection }, previous)
  await new Promise((resolve) => setImmediate(resolve))
  pendingVersions.get('game-3.4')({ major: 3, minor: 4, release: 0 })
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(section.state.buttonLabel, '启动游戏  3.4')
  pendingVersions.get('game-4.7')({ major: 4, minor: 7, release: 0 })
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(section.state.buttonLabel, '启动游戏  3.4')
  const old = { ...selection }
  selection.show_version = false
  changed({ ...selection }, old)
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(section.state.buttonLabel, '启动游戏')
  section.componentWillUnmount()
  assert(unsubscribed)
  const { wallpaperQueue } = moduleAt('src/utils/wallpapers.ts', () => ({}))
  for (const count of [0, 1, 2, 3, 10, 11, 37, 1000]) {
    let previous = -1
    for (let round = 0; round < 50; round++) {
      const playlist = Array.from(wallpaperQueue(count, previous))
      assert.equal(playlist.length, count)
      assert.deepEqual([...playlist].sort((a, b) => a - b), Array.from({ length: count }, (_, index) => index))
      if (count > 1) assert.notEqual(playlist[0], previous, 'Consecutive rounds must not repeat the last wallpaper')
      previous = playlist[playlist.length - 1] ?? -1
    }
  }
  const Background = moduleAt('src/ui/components/Background.tsx', (name) => {
    if (name === 'react') return React
    if (name === '../../utils/wallpapers') return { wallpaperQueue }
    return {}
  }).default
  for (const wallpapers of [[], ['only-one.png']]) {
    const background = new Background({})
    background.live = true
    background.state.wallpapers = wallpapers
    background.schedule()
    assert.equal(background.timer, undefined, 'Empty and single-image playlists should not start timers')
  }
  console.log('PASS: wallpaper shuffle for 0/1/2/3/10/11/37/1000 images, complete rounds, no repeat across rounds, no empty/single-image timers.')
  console.log(
    'PASS: concurrent configuration updates, default migration, cached reads, snapshot isolation, failed-save recovery, corrupt-file backup, first launch, translation caching/fallback, controlled empty input, configuration notifications, version switching with out-of-order responses, dropdown viewport placement.'
  )
})().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
