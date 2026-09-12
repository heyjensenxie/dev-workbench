import { describe, expect, it } from 'vitest'
import {
  AppError,
  clampLogLimit,
  createServiceId,
  formatBytes,
  formatUptime,
  isAbsolutePath,
  LOG_LIMIT_MAX,
  LOG_LIMIT_MIN,
  parseSettings,
  validateDevService,
} from './index'
import type { DevService } from './index'

function service(overrides: Partial<DevService> = {}): DevService {
  return { id: 'service-1', projectId: 'project-1', name: 'api', command: 'pnpm', ...overrides }
}

describe('validateDevService', () => {
  it('accepts a complete service', () => {
    expect(validateDevService(service({ cwd: '/srv/app', args: ['dev'], port: 5173 }))).toEqual([])
  })

  it('reports missing required fields', () => {
    expect(validateDevService(service({ name: '   ', command: '' })))
      .toEqual(['nameRequired', 'commandRequired'])
  })

  it('rejects an over-long name', () => {
    expect(validateDevService(service({ name: 'a'.repeat(81) }))).toEqual(['nameTooLong'])
    expect(validateDevService(service({ name: 'a'.repeat(80) }))).toEqual([])
  })

  it('accepts a project-relative working directory', () => {
    expect(validateDevService(service({ cwd: 'apps/web' }))).toEqual([])
  })

  it('accepts absolute paths from every platform', () => {
    for (const cwd of ['/srv/app', 'C:\\projects\\web', 'c:/projects/web', '\\\\server\\share']) {
      expect(validateDevService(service({ cwd })), cwd).toEqual([])
    }
  })

  it('rejects a service depending on itself', () => {
    expect(validateDevService(service({ dependencies: ['service-1'] }))).toEqual(['selfDependency'])
    expect(validateDevService(service({ dependencies: ['service-2'] }))).toEqual([])
  })

  it('collects every problem at once', () => {
    expect(validateDevService({ ...service(), name: '', command: ' ', cwd: 'relative' }))
      .toEqual(['nameRequired', 'commandRequired'])
  })
})

describe('isAbsolutePath', () => {
  it('distinguishes absolute from relative paths', () => {
    expect(isAbsolutePath('/srv/app')).toBe(true)
    expect(isAbsolutePath('C:\\app')).toBe(true)
    expect(isAbsolutePath('\\\\server\\share')).toBe(true)
    expect(isAbsolutePath('apps/web')).toBe(false)
    expect(isAbsolutePath('./apps')).toBe(false)
    expect(isAbsolutePath('')).toBe(false)
  })
})

describe('createServiceId', () => {
  it('generates unique identifiers', () => {
    const ids = new Set(Array.from({ length: 50 }, () => createServiceId()))
    expect(ids.size).toBe(50)
    for (const id of ids) expect(id.length).toBeGreaterThan(6)
  })
})

describe('AppError', () => {
  it('passes AppError instances through', () => {
    const original = new AppError('NOT_FOUND', 'missing')
    expect(AppError.from(original)).toBe(original)
  })

  it('adopts serialized native errors', () => {
    const error = AppError.from({ code: 'VALIDATION_ERROR', message: 'bad input' })
    expect(error.code).toBe('VALIDATION_ERROR')
    expect(error.message).toBe('bad input')
  })

  it('falls back for unknown throwables', () => {
    const error = AppError.from(new Error('boom'))
    expect(error.code).toBe('UNKNOWN_ERROR')
    expect(error.message).toBe('An unexpected error occurred')
  })
})

describe('parseSettings', () => {
  it('keeps usable fields and drops the rest', () => {
    expect(parseSettings({
      theme: 'light',
      locale: 'de-DE',
      logLimit: 2500,
      confirmBeforeKill: false,
      unknown: 'ignored',
    })).toEqual({ theme: 'light', logLimit: 2500, confirmBeforeKill: false })
  })

  it('ignores unusable values so callers keep their defaults', () => {
    expect(parseSettings({})).toEqual({})
    expect(parseSettings({ theme: 'blue', logLimit: 'many', confirmBeforeKill: 1 })).toEqual({})
    expect(parseSettings({ logLimit: Number.NaN })).toEqual({})
  })

  it('rounds and clamps the log limit', () => {
    expect(parseSettings({ logLimit: 99.6 })).toEqual({ logLimit: LOG_LIMIT_MIN })
    expect(parseSettings({ logLimit: 1_000_000 })).toEqual({ logLimit: LOG_LIMIT_MAX })
  })

  it('parses the startup project preference', () => {
    expect(parseSettings({ openLastProject: false })).toEqual({ openLastProject: false })
    expect(parseSettings({ openLastProject: 'yes' })).toEqual({})
  })
})

describe('clampLogLimit', () => {
  it('keeps values inside the supported range', () => {
    expect(clampLogLimit(0)).toBe(LOG_LIMIT_MIN)
    expect(clampLogLimit(1000)).toBe(1000)
    expect(clampLogLimit(Number.MAX_SAFE_INTEGER)).toBe(LOG_LIMIT_MAX)
  })
})

describe('formatBytes', () => {
  it('scales the unit to the value', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(undefined)).toBe('0 B')
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2048)).toBe('2.0 KB')
    expect(formatBytes(1024 * 1024 * 1024 * 2.5)).toBe('2.5 GB')
  })
})

describe('formatUptime', () => {
  const now = 1_700_000_000_000

  it('formats seconds, minutes, hours and days', () => {
    expect(formatUptime(now / 1000 - 45, now)).toBe('45s')
    expect(formatUptime(now / 1000 - 90, now)).toBe('1m')
    expect(formatUptime(now / 1000 - 3 * 3600 - 120, now)).toBe('3h 2m')
    expect(formatUptime(now / 1000 - 50 * 3600, now)).toBe('2d 2h')
  })

  it('handles absent or future timestamps', () => {
    expect(formatUptime(undefined, now)).toBe('—')
    expect(formatUptime(now / 1000 + 500, now)).toBe('0s')
  })
})
