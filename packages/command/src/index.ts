import { AppError } from '@dev-workbench/shared'

export interface WorkbenchContext {
  activeProjectId: string | undefined
}

export interface ServiceContainer {
  get<T>(key: string): T
}

export interface CommandContext {
  workbench: WorkbenchContext
  services: ServiceContainer
}

export interface Command<TInput = unknown, TOutput = unknown> {
  id: string
  title: string
  description?: string
  category?: string
  execute(input: TInput, context: CommandContext): Promise<TOutput>
}

export class CommandRegistry {
  private readonly commands = new Map<string, Command<unknown, unknown>>()

  register<TInput, TOutput>(command: Command<TInput, TOutput>): () => void {
    if (this.commands.has(command.id)) {
      throw new AppError('CONFLICT', `Command already registered: ${command.id}`)
    }
    this.commands.set(command.id, command as Command<unknown, unknown>)
    return () => this.unregister(command.id)
  }

  unregister(id: string): boolean {
    return this.commands.delete(id)
  }

  list(): Command[] {
    return [...this.commands.values()].sort((a, b) => a.title.localeCompare(b.title))
  }

  async execute<TInput, TOutput>(id: string, input: TInput, context: CommandContext): Promise<TOutput> {
    const command = this.commands.get(id)
    if (!command) throw new AppError('NOT_FOUND', `Unknown command: ${id}`)
    try {
      return await command.execute(input, context) as TOutput
    } catch (error) {
      const cause = AppError.from(error)
      throw new AppError('COMMAND_ERROR', `Command failed: ${id}`, { commandId: id, cause }, { cause })
    }
  }
}

export function fuzzyMatch(query: string, value: string): boolean {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return true
  let cursor = 0
  for (const char of value.toLocaleLowerCase()) {
    if (char === needle[cursor]) cursor += 1
    if (cursor === needle.length) return true
  }
  return false
}
