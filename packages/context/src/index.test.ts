import { describe, expect, it, vi } from 'vitest'
import { ContextStore } from './index'
import type { ProjectContext } from '@dev-workbench/shared'

function context(id: string): ProjectContext {
  return {
    id,
    name: id,
    path: `/projects/${id}`,
    languages: ['Rust'],
    frameworks: [],
    packageManagers: ['Cargo'],
    detectedFiles: [],
    scripts: [],
    suggestedServices: [],
    composeServices: [],
    monorepo: false,
    scannedAt: 1,
  }
}

describe('ContextStore', () => {
  it('starts empty', () => {
    const store = new ContextStore()
    expect(store.snapshot).toEqual({ projects: {} })
  })

  it('stores project contexts by id', () => {
    const store = new ContextStore()
    store.setProject(context('alpha'))
    store.setProject(context('beta'))
    expect(Object.keys(store.snapshot.projects)).toEqual(['alpha', 'beta'])
    expect(store.snapshot.projects['alpha']?.path).toBe('/projects/alpha')
  })

  it('tracks and clears the active project', () => {
    const store = new ContextStore()
    store.setActiveProject('alpha')
    expect(store.snapshot.activeProjectId).toBe('alpha')
    store.setActiveProject(undefined)
    expect(store.snapshot.activeProjectId).toBeUndefined()
    expect(store.snapshot.projects).toEqual({})
  })

  it('notifies subscribers and supports unsubscribing', () => {
    const store = new ContextStore()
    const listener = vi.fn()
    const unsubscribe = store.subscribe(listener)

    store.setProject(context('alpha'))
    store.setActiveProject('alpha')
    expect(listener).toHaveBeenCalledTimes(2)
    expect(listener.mock.calls[1]?.[0]).toMatchObject({ activeProjectId: 'alpha' })

    unsubscribe()
    store.setProject(context('beta'))
    expect(listener).toHaveBeenCalledTimes(2)
  })

  it('publishes immutable snapshots', () => {
    const store = new ContextStore()
    const before = store.snapshot
    store.setProject(context('alpha'))
    expect(store.snapshot).not.toBe(before)
    expect(before.projects).toEqual({})
  })
})
