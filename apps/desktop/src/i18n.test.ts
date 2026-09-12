import { afterEach, describe, expect, it } from 'vitest'
import { DATABASE_MESSAGE_CODES } from '@dev-workbench/shared'
import { messageCatalog, setLocale, t, translateDatabaseMessage, translateSqlWarning } from './i18n'

afterEach(() => {
  setLocale('en-US')
})

describe('message catalog', () => {
  it('keeps the Chinese and English catalogs in sync', () => {
    expect(Object.keys(messageCatalog['zh-CN']).sort()).toEqual(Object.keys(messageCatalog['en-US']).sort())
  })

  it('never ships an empty translation', () => {
    const empty = Object.entries(messageCatalog)
      .flatMap(([locale, catalog]) => Object.entries(catalog)
        .filter(([, value]) => typeof value !== 'string' || !value.trim())
        .map(([key]) => `${locale}.${key}`))
    expect(empty).toEqual([])
  })

  it('interpolates named placeholders per locale', () => {
    expect(t('dbShowingFirstRows', { count: 25 })).toBe('Showing first 25 rows')
    setLocale('zh-CN')
    expect(t('dbShowingFirstRows', { count: 25 })).toBe('仅显示前 25 行')
  })

  it('leaves unknown placeholders untouched', () => {
    expect(t('dbRowSummary', { rows: 3 })).toBe('3 rows · {ms} ms')
  })
})

describe('locale-neutral database messages', () => {
  it('translates workbench-generated codes', () => {
    expect(translateDatabaseMessage(DATABASE_MESSAGE_CODES.connectedSqlite)).toBe('Connected to SQLite')
    setLocale('zh-CN')
    expect(translateDatabaseMessage(DATABASE_MESSAGE_CODES.connectedSqlite)).toBe('已连接到 SQLite')
    expect(translateDatabaseMessage(DATABASE_MESSAGE_CODES.limitReached)).toBe('仅显示部分行')
  })

  it('labels every database runtime code', () => {
    for (const code of Object.values(DATABASE_MESSAGE_CODES)) {
      expect(translateDatabaseMessage(code)).not.toBe(code)
    }
  })

  it('passes engine errors and empty input through unchanged', () => {
    expect(translateDatabaseMessage("You have an error in your SQL syntax near 'selekt'")).toBe("You have an error in your SQL syntax near 'selekt'")
    expect(translateDatabaseMessage(undefined)).toBeUndefined()
  })

  it('localizes every SQL safety warning kind', () => {
    const kinds = ['drop', 'truncate', 'alter', 'update-without-where', 'delete-without-where'] as const
    for (const kind of kinds) {
      const english = translateSqlWarning(kind)
      setLocale('zh-CN')
      const chinese = translateSqlWarning(kind)
      expect(english.title).not.toBe('')
      expect(english.detail).not.toBe('')
      expect(chinese.title).not.toBe('')
      expect(chinese.title).not.toBe(english.title)
      setLocale('en-US')
    }
  })
})
