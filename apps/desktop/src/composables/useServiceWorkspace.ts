import type { DevService, SuggestedService } from '@dev-workbench/shared'
import { ref } from 'vue'
import { useI18n } from '../i18n'
import { useWorkbenchStore } from '../stores/workbench'

/**
 * Shared service workspace behaviour: the editor dialog, the batch status
 * message, and the handlers both the project and services pages need.
 */
export function useServiceWorkspace() {
  const store = useWorkbenchStore()
  const { t } = useI18n()
  const editing = ref<DevService>()
  const editorOpen = ref(false)
  const batchError = ref<string>()

  function addService(): void {
    editing.value = store.newServiceDraft()
    editorOpen.value = true
  }

  function editService(service: DevService): void {
    editing.value = {
      ...service,
      args: [...(service.args ?? [])],
      dependencies: [...(service.dependencies ?? [])],
    }
    editorOpen.value = true
  }

  function closeEditor(): void {
    editorOpen.value = false
    editing.value = undefined
  }

  async function saveService(service: DevService): Promise<void> {
    batchError.value = undefined
    if (await store.saveService(service)) closeEditor()
  }

  async function importSuggestion(suggestion: SuggestedService): Promise<void> {
    batchError.value = undefined
    await store.importSuggestion(suggestion)
  }

  function dismiss(): void {
    batchError.value = undefined
    store.dismissError()
  }

  async function runBatch(action: 'start' | 'stop'): Promise<void> {
    batchError.value = undefined
    const outcome = action === 'start' ? await store.startAll() : await store.stopAll()
    if (!outcome?.failed.length) return
    const names = outcome.failed.map((failure) =>
      store.services.find((service) => service.id === failure.serviceId)?.name ?? failure.serviceId)
    batchError.value = `${t('batchPartialFailure')}: ${names.join(', ')}`
  }

  return {
    store, editing, editorOpen, batchError,
    addService, editService, closeEditor, saveService, importSuggestion, dismiss, runBatch,
  }
}
