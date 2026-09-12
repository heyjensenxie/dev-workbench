import { describe, expect, it, vi } from 'vitest'
import { CommandRegistry, fuzzyMatch, type CommandContext } from './index'

const context = { workbench: { activeProjectId: undefined }, services: { get: vi.fn() } } satisfies CommandContext

describe('CommandRegistry', () => {
  it('registers, lists and executes commands', async () => {
    const registry = new CommandRegistry()
    registry.register({ id: 'service.restart', title: 'Restart Service', execute: async (name: string) => `restarted ${name}` })
    expect(registry.list().map(({ id }) => id)).toEqual(['service.restart'])
    await expect(registry.execute('service.restart', 'api', context)).resolves.toBe('restarted api')
  })

  it('rejects duplicate ids', () => {
    const registry = new CommandRegistry()
    const command = { id: 'project.open', title: 'Open Project', execute: async () => undefined }
    registry.register(command)
    expect(() => registry.register(command)).toThrow('already registered')
  })
})

describe('fuzzyMatch', () => {
  it('matches ordered characters regardless of case', () => {
    expect(fuzzyMatch('rs', 'Restart Service')).toBe(true)
    expect(fuzzyMatch('xyz', 'Restart Service')).toBe(false)
  })
})
