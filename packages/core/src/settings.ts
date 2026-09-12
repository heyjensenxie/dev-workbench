import type { WorkbenchSettings } from '@dev-workbench/shared'
import { parseSettings } from '@dev-workbench/shared'

/** The native capabilities the settings surface needs. */
export interface SettingsBridge {
  getSettings(): Promise<Record<string, unknown>>
  setSetting(key: string, value: unknown): Promise<void>
}

export class SettingsService {
  constructor(private readonly bridge: SettingsBridge) {}

  /** Only the persisted fields come back; callers keep their own defaults. */
  async load(): Promise<Partial<WorkbenchSettings>> {
    return parseSettings(await this.bridge.getSettings())
  }

  async save(key: keyof WorkbenchSettings, value: unknown): Promise<void> {
    await this.bridge.setSetting(key, value)
  }

  /** Persists several fields in order, so a failure leaves a known prefix saved. */
  async saveAll(settings: Partial<WorkbenchSettings>): Promise<void> {
    for (const [key, value] of Object.entries(settings)) {
      await this.bridge.setSetting(key, value)
    }
  }
}
