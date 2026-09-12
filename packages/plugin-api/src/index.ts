import type { Command } from '@dev-workbench/command'

export interface WorkbenchPlugin {
  id: string
  name: string
  activate(): Promise<void> | void
  deactivate?(): Promise<void> | void
  commands?: Command[]
}

