import type { DatabaseContext, ProjectContext } from '@dev-workbench/shared'

export interface WorkbenchState {
  activeProjectId?: string
  projects: Record<string, ProjectContext>
  database?: DatabaseContext | undefined
}

export class ContextStore {
  private state: WorkbenchState = { projects: {} }
  private readonly listeners = new Set<(state: Readonly<WorkbenchState>) => void>()

  get snapshot(): Readonly<WorkbenchState> {
    return this.state
  }

  setProject(context: ProjectContext): void {
    this.state = { ...this.state, projects: { ...this.state.projects, [context.id]: context } }
    this.emit()
  }

  setActiveProject(projectId?: string): void {
    this.state = projectId ? { ...this.state, activeProjectId: projectId } : { projects: this.state.projects }
    this.emit()
  }

  setDatabase(database?: DatabaseContext): void {
    if (database) this.state = { ...this.state, database }
    else {
      const { database: _database, ...state } = this.state
      this.state = state
    }
    this.emit()
  }

  subscribe(listener: (state: Readonly<WorkbenchState>) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private emit(): void {
    this.listeners.forEach((listener) => listener(this.state))
  }
}
