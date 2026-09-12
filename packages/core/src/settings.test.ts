import { describe, expect, it, vi } from 'vitest'
import { SettingsService, type SettingsBridge } from './settings'

function createBridge(overrides: Partial<SettingsBridge> = {}): SettingsBridge {
  return {
    getSettings: vi.fn(async () => ({}) as Record<string, unknown>),
    setSetting: vi.fn(async () => undefined),
    ...overrides,
  }
}

describe('SettingsService', () => {
  it('returns only the settings that are present and usable', async () => {
    const bridge = createBridge({
      getSettings: vi.fn(async () => ({
        theme: 'light',
        locale: 'klingon',
        logLimit: 12,
        confirmBeforeKill: 'yes',
      })),
    })
    await expect(new SettingsService(bridge).load()).resolves.toEqual({
      theme: 'light',
      logLimit: 100,
    })
  })

  it('returns an empty patch when nothing was stored', async () => {
    await expect(new SettingsService(createBridge()).load()).resolves.toEqual({})
  })

  it('saves a single field', async () => {
    const bridge = createBridge()
    await new SettingsService(bridge).save('theme', 'light')
    expect(bridge.setSetting).toHaveBeenCalledWith('theme', 'light')
  })

  it('saves several fields in order', async () => {
    const calls: string[] = []
    const bridge = createBridge({
      setSetting: vi.fn(async (key: string) => { calls.push(key) }),
    })
    await new SettingsService(bridge).saveAll({ theme: 'dark', logLimit: 500 })
    expect(calls).toEqual(['theme', 'logLimit'])
  })
})
