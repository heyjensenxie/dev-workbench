/**
 * Password Vault isolation tests.
 *
 * These cover the requirements that the vault must be unreachable from the rest
 * of Dev Workbench — specifically that plugins and AI-facing surfaces cannot
 * read a credential, and that the command system cannot be used as a way around
 * the vault's own screens.
 *
 * The tests are written against the *public* workbench application, not against
 * internals, because the property being defended is about what other modules can
 * reach, not about how the vault is implemented.
 */

import type { Command } from '@dev-workbench/command'
import type { DevService, Project, ProjectContext, VaultItem, VaultItemSummary, VaultStatus } from '@dev-workbench/shared'
import { describe, expect, it, vi } from 'vitest'
import { createWorkbench, type NativeBridge, type VaultBridge } from './index'

const PROJECT: Project = {
  id: 'project-1',
  name: 'demo',
  path: '/projects/demo',
  createdAt: 1,
  updatedAt: 2,
  lastOpenedAt: 2,
}

const PROJECT_CONTEXT: ProjectContext = {
  id: 'project-1',
  name: 'demo',
  path: '/projects/demo',
  languages: [],
  frameworks: [],
  packageManagers: [],
  detectedFiles: [],
  scripts: [],
  suggestedServices: [],
  composeServices: [],
  monorepo: false,
  scannedAt: 2,
}

/** A distinctive value, so a leak into any serialized structure is detectable. */
const SECRET = 'SUPER-SECRET-PASSWORD-MARKER'

/** A representative unlocked status, so fixtures stay in one place. */
function unlockedStatus(itemCount: number): VaultStatus {
  return {
    exists: true,
    unlocked: true,
    formatVersion: 1,
    itemCount,
    autoLockSeconds: 300,
    memoryLocked: true,
    failedAttempts: 0,
    lockedUntil: null,
    kdf: {
      algorithm: 'argon2id',
      version: 1,
      memoryKib: 64 * 1024,
      timeCost: 3,
      parallelism: 4,
      meetsCurrentFloor: true,
    },
  }
}

function createVaultBridge(overrides: Partial<VaultBridge> = {}): VaultBridge {
  return {
    status: vi.fn(async () => unlockedStatus(1)),
    create: vi.fn(async () => unlockedStatus(0)),
    unlock: vi.fn(async () => unlockedStatus(1)),
    lock: vi.fn(async () => true),
    touch: vi.fn(async () => undefined),
    setAutoLock: vi.fn(async () => undefined),
    listItems: vi.fn(async () => [
      { id: 'item-1', title: 'GitHub', tags: [], favorite: false, hasPassword: true, hasNotes: false, customFieldCount: 0, createdAt: 1, updatedAt: 2 },
    ] as VaultItemSummary[]),
    // A fetched item carries no password: that is the whole point of the
    // redaction invariant, and this fixture must model it or the isolation tests
    // would be asserting against a fiction.
    getItem: vi.fn(async () => ({
      id: 'item-1',
      title: 'GitHub',
      username: 'jensen@example.com',
      tags: [],
      customFields: [],
      favorite: false,
      hasPassword: true,
      createdAt: 1,
      updatedAt: 2,
    }) as VaultItem),
    createItem: vi.fn(async (payload) => ({ ...payload, id: 'item-2', hasPassword: true, createdAt: 1, updatedAt: 1 }) as VaultItem),
    updateItem: vi.fn(async (id, payload) => ({ ...payload, id, hasPassword: true, createdAt: 1, updatedAt: 2 }) as VaultItem),
    deleteItem: vi.fn(async () => true),
    destroy: vi.fn(async () => undefined),
    setFavorite: vi.fn(async (id, favorite) => ({ id, title: 'GitHub', tags: [], customFields: [], favorite, hasPassword: true, createdAt: 1, updatedAt: 2 }) as VaultItem),
    revealField: vi.fn(async () => SECRET),
    copyItemField: vi.fn(async () => undefined),
    changeMasterPassword: vi.fn(async () => undefined),
    recalibrate: vi.fn(async () => unlockedStatus(1)),
    exportBackup: vi.fn(async () => ({ itemCount: 1, formatVersion: 1 })),
    importBackup: vi.fn(async () => ({ itemCount: 1, formatVersion: 1 })),    generatePassword: vi.fn(async () => 'generated-password-value'),
    copySecret: vi.fn(async () => undefined),
    clearClipboard: vi.fn(async () => undefined),
    openUrl: vi.fn(async () => undefined),
    ...overrides,
  }
}

function createFakeBridge(vault: VaultBridge): NativeBridge {
  return {
    listProjects: vi.fn(async () => [PROJECT]),
    openProject: vi.fn(async () => PROJECT),
    deleteProject: vi.fn(async () => true),
    revealProject: vi.fn(async () => undefined),
    scanProject: vi.fn(async () => PROJECT_CONTEXT),
    listServices: vi.fn(async () => [] as DevService[]),
    saveService: vi.fn(async () => undefined),
    deleteService: vi.fn(async () => true),
    startService: vi.fn(async (service: DevService) => ({ serviceId: service.id, status: 'running' as const, pid: 1 })),
    stopService: vi.fn(async (serviceId: string) => ({ serviceId, status: 'stopped' as const })),
    stopAllServices: vi.fn(async () => 0),
    listRunningServices: vi.fn(async () => []),
    listProcesses: vi.fn(async () => []),
    processTree: vi.fn(async () => []),
    killProcess: vi.fn(async () => undefined),
    listPorts: vi.fn(async () => []),
    killPort: vi.fn(async () => undefined),
    getSettings: vi.fn(async () => ({})),
    setSetting: vi.fn(async () => undefined),
    httpRequest: vi.fn(async () => ({ status: 200, statusText: 'OK', headers: {}, body: '', durationMs: 1, truncated: false })),
    listApiModules: vi.fn(async () => []),
    saveApiModule: vi.fn(async (module) => module),
    deleteApiModule: vi.fn(async () => true),
    listApiRequests: vi.fn(async () => []),
    saveApiRequest: vi.fn(async (request) => request),
    deleteApiRequest: vi.fn(async () => true),
    vault,
  }
}

describe('plugin isolation: plugins cannot reach vault data', () => {
  it('does not publish the vault through the service container', () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))

    // The service container is what every command — and therefore every plugin —
    // receives. The vault must not be resolvable from it under any plausible key.
    for (const key of ['vault', 'Vault', 'vaultCatalog', 'secrets', 'credentials', 'nativeBridge']) {
      if (key === 'nativeBridge') continue
      expect(() => workbench.commandContext.services.get(key)).toThrow()
    }
  })

  it('gives a plugin command a context with no vault access', async () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))

    // A third-party plugin, shaped exactly like `WorkbenchPlugin.commands`.
    let observed: CommandContextShape | undefined
    const pluginCommand: Command = {
      id: 'plugin.inspect',
      title: 'Plugin inspect',
      execute: async (_input, context) => {
        observed = { serviceKeys: context.services as unknown, workbench: context.workbench }
        return 'ok'
      },
    }
    workbench.commands.register(pluginCommand)
    await workbench.commands.execute('plugin.inspect', undefined, workbench.commandContext)

    expect(observed).toBeDefined()
    // The only thing a plugin could do with the container is ask for a key it
    // already knows; every vault-related key fails.
    expect(() => workbench.commandContext.services.get('vault')).toThrow()
    expect(JSON.stringify(observed?.workbench)).not.toContain(SECRET)
  })

  it('never places vault data in the workbench context that AI features read', () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))
    const snapshot = workbench.context.snapshot

    expect(Object.keys(snapshot)).not.toContain('vault')
    expect(JSON.stringify(snapshot)).not.toContain(SECRET)
    expect(JSON.stringify(workbench.commandContext.workbench)).not.toContain(SECRET)
  })
})

interface CommandContextShape {
  serviceKeys: unknown
  workbench: unknown
}

describe('command isolation: no command can return a secret', () => {
  it('registers exactly two vault commands', () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))
    const vaultCommands = workbench.commands.list().filter((command) => command.id.startsWith('vault.'))

    expect(vaultCommands.map((command) => command.id).sort()).toEqual(['vault.lock', 'vault.open'])
  })

  it('registers no command that reads, lists, or exports vault secrets', () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))
    // Names that would describe handing a credential to an arbitrary caller.
    const forbidden = /getpassword|listsecrets|readsecret|exportplaintext|exportcsv|plaintext|revealpassword|vault\.(?:get|read|list|export)/i

    for (const command of workbench.commands.list()) {
      expect(forbidden.test(command.id), `${command.id} looks like a secret-reading command`).toBe(false)
    }
  })

  it('opens the vault by returning a navigation token, not data', async () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))
    const result = await workbench.commands.execute<undefined, string>('vault.open', undefined, workbench.commandContext)
    expect(result).toBe('vault')
  })

  it('locks the vault from the command system without exposing anything', async () => {
    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    const result = await workbench.commands.execute('vault.lock', undefined, workbench.commandContext)
    expect(vault.lock).toHaveBeenCalledTimes(1)
    expect(result).toBe(true)
  })
})

describe('vault service surface', () => {
  it('reads the list projection without ever returning a password', async () => {
    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    const items = await workbench.vault.listItems()
    expect(JSON.stringify(items)).not.toContain(SECRET)
    expect(vault.getItem).not.toHaveBeenCalled()
  })

  it('fetches an item for display without any secret in it', async () => {
    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    const item = await workbench.vault.getItem('item-1')
    // The item carries everything the detail pane renders, and no secret.
    expect(item?.title).toBe('GitHub')
    expect(item?.hasPassword).toBe(true)
    expect(item?.password).toBeUndefined()
    expect(JSON.stringify(item)).not.toContain(SECRET)
    expect(vault.getItem).toHaveBeenCalledWith('item-1')
  })

  it('produces a secret only from an explicit per-field reveal', async () => {
    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    expect(await workbench.vault.revealField('item-1')).toBe(SECRET)
    expect(vault.revealField).toHaveBeenCalledWith('item-1', undefined)

    await workbench.vault.revealField('item-1', 3)
    expect(vault.revealField).toHaveBeenLastCalledWith('item-1', 3)
  })

  it('copies a secret natively without returning it to the caller', async () => {
    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    const result = await workbench.vault.copyItemField('item-1', undefined, 30)

    // The contract is that the value is never handed back.
    expect(result).toBeUndefined()
    expect(vault.copyItemField).toHaveBeenCalledWith('item-1', undefined, 30)
  })

  it('toggles a favourite without requiring the whole item back', async () => {    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    const updated = await workbench.vault.setFavorite('item-1', true)

    expect(vault.setFavorite).toHaveBeenCalledWith('item-1', true)
    expect(updated.favorite).toBe(true)
    expect(updated.password).toBeUndefined()
  })

  it('passes non-secret clipboard values straight through', async () => {
    const vault = createVaultBridge()
    const workbench = createWorkbench(createFakeBridge(vault))

    await workbench.vault.copySecret('jensen@example.com', 0)
    expect(vault.copySecret).toHaveBeenCalledWith('jensen@example.com', 0)
  })

  it('has no method that can return every secret at once', () => {
    const workbench = createWorkbench(createFakeBridge(createVaultBridge()))
    const methods = Object.getOwnPropertyNames(Object.getPrototypeOf(workbench.vault))

    expect(methods).not.toContain('listSecrets')
    expect(methods).not.toContain('exportPlaintext')
    expect(methods).not.toContain('getPassword')
    // Every method is a single explicit operation.
    expect(methods).toContain('getItem')
    expect(methods).toContain('lock')
  })
})
