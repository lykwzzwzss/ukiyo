import { invoke } from '@tauri-apps/api'
import React from 'react'
import { getConfigOption } from './configuration'

type Dictionary = { [key: string]: unknown }
const dictionaries = new Map<string, Promise<Dictionary>>()
function dictionary(lang: string): Promise<Dictionary> {
  const cached = dictionaries.get(lang)
  if (cached) return cached
  const pending = invoke<string>('get_lang', { lang })
    .then((raw) => JSON.parse(raw || '{}') as Dictionary)
    .catch((error) => {
      dictionaries.delete(lang)
      throw error
    })
  dictionaries.set(lang, pending)
  return pending
}

function lookup(dict: Dictionary, key: string): string | undefined {
  let value: unknown = dict
  for (const segment of key.split('.')) {
    if (!value || typeof value !== 'object') return undefined
    value = (value as Dictionary)[segment]
  }
  return typeof value === 'string' ? value : undefined
}
export async function translate(text: string): Promise<string> {
  const lang = (await getConfigOption('language')) || 'en'
  const [selected, fallback] = await Promise.all([dictionary(lang), dictionary('en')])
  return lookup(selected, text) || lookup(fallback, text) || text
}
export default class Tr extends React.Component<{ text: string }, { translated: string }> {
  state = { translated: '' }
  private mounted = false
  private revision = 0
  componentDidMount() {
    this.mounted = true
    void this.refresh()
  }
  componentDidUpdate(previous: Readonly<{ text: string }>) {
    if (previous.text !== this.props.text) void this.refresh()
  }
  componentWillUnmount() {
    this.mounted = false
    this.revision++
  }
  private async refresh() {
    const revision = ++this.revision
    try {
      const translated = await translate(this.props.text)
      if (this.mounted && revision === this.revision) this.setState({ translated })
    } catch (error) {
      console.error(error)
    }
  }
  render() {
    return this.state.translated || this.props.text
  }
}
export async function getLanguages(): Promise<{ [key: string]: string }[]> {
  const files = await invoke<{ [key: string]: string }>('get_languages')
  return Object.keys(files).flatMap((file) => {
    try {
      const parsed = JSON.parse(files[file])
      return parsed.lang_name ? [{ [file.replace(/\.json$/, '')]: String(parsed.lang_name) }] : []
    } catch {
      return []
    }
  })
}
