<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Check, ChevronLeft, ChevronRight, Clock3, Copy, Plus, Send, Trash2, X } from 'lucide-vue-next'
import type { ApiModule, HttpMethod, HttpRequest, HttpResponse, SavedApiRequest } from '@dev-workbench/shared'
import { workbench } from '../services/workbench'
import { useI18n } from '../i18n'

type ApiTab = 'params' | 'headers' | 'body'
type ResponseTab = 'pretty' | 'raw' | 'headers'
interface KeyValueRow { key: string; value: string; enabled: boolean }
interface RequestHistory { method: HttpMethod; url: string; status: number; durationMs: number; at: number }

const { t } = useI18n()
const method = ref<HttpMethod>('GET')
const url = ref('https://httpbin.org/get')
const activeTab = ref<ApiTab>('params')
const responseTab = ref<ResponseTab>('pretty')
const queryRows = ref<KeyValueRow[]>([{ key: '', value: '', enabled: true }])
const headerRows = ref<KeyValueRow[]>([{ key: 'Accept', value: 'application/json', enabled: true }])
const body = ref('')
const environment = ref('Local')
const environmentText = ref('')
const timeoutMs = ref(30000)
const loading = ref(false)
const error = ref('')
const response = ref<HttpResponse>()
const history = ref<RequestHistory[]>([])
const copied = ref(false)
const modules = ref<ApiModule[]>([])
const savedRequests = ref<SavedApiRequest[]>([])
const selectedModuleId = ref<string>()
const currentRequestId = ref<string>()
const saveName = ref('')
const moduleName = ref('')
const apiSidebarCollapsed = ref(initialApiSidebarCollapsed())
const saveState = ref<'idle' | 'saved'>('idle')

function initialApiSidebarCollapsed(): boolean {
  try { return typeof localStorage !== 'undefined' && localStorage.getItem('api-sidebar-collapsed') === 'true' } catch { return false }
}
function toggleApiSidebar(): void {
  apiSidebarCollapsed.value = !apiSidebarCollapsed.value
  try { localStorage.setItem('api-sidebar-collapsed', String(apiSidebarCollapsed.value)) } catch { /* Optional UI preference. */ }
}

const responseBody = computed(() => {
  if (!response.value) return ''
  try { return JSON.stringify(JSON.parse(response.value.body) as unknown, null, 2) } catch { return response.value.body }
})
const responseTone = computed(() => {
  const status = response.value?.status ?? 0
  return status >= 200 && status < 300 ? 'success' : status >= 400 ? 'danger' : 'warning'
})

function addRow(rows: KeyValueRow[]): void { rows.push({ key: '', value: '', enabled: true }) }
function removeRow(rows: KeyValueRow[], index: number): void { if (rows.length > 1) rows.splice(index, 1) }
function parseEnvironment(): Record<string, string> {
  const values: Record<string, string> = {}
  for (const line of environmentText.value.split(/\r?\n/)) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const separator = trimmed.indexOf('=')
    if (separator <= 0) throw new Error(t('apiErrorInvalidEnvVar', { line: trimmed }))
    const key = trimmed.slice(0, separator).trim()
    values[key] = trimmed.slice(separator + 1).trim().replace(/^(['"])(.*)\1$/, '$2')
  }
  return values
}
function resolveVariables(value: string, values: Record<string, string>): string {
  return value.replace(/\{\{\s*([A-Za-z_][\w.-]*)\s*\}\}/g, (full, key: string) => {
    if (!(key in values)) throw new Error(t('apiErrorUndefinedEnvVar', { name: key }))
    return values[key]!
  })
}
function activeHeaders(values: Record<string, string>): Record<string, string> {
  const headers: Record<string, string> = {}
  for (const row of headerRows.value) {
    if (!row.enabled || !row.key.trim()) continue
    headers[row.key.trim()] = resolveVariables(row.value, values)
  }
  return headers
}
function buildRequest(): HttpRequest {
  const values = parseEnvironment()
  const target = new URL(resolveVariables(url.value.trim(), values))
  for (const row of queryRows.value) {
    if (row.enabled && row.key.trim()) target.searchParams.append(row.key.trim(), resolveVariables(row.value, values))
  }
  const headers = activeHeaders(values)
  const resolvedBody = body.value.trim() && !['GET', 'HEAD'].includes(method.value) ? resolveVariables(body.value, values) : undefined
  return { method: method.value, url: target.toString(), headers, timeoutMs: Math.min(120000, Math.max(1000, timeoutMs.value)), ...(resolvedBody ? { body: resolvedBody } : {}) }
}
function buildSavedUrl(): string {
  const raw = url.value.trim()
  const params = queryRows.value.filter((row) => row.enabled && row.key.trim()).map((row) => [row.key.trim(), row.value])
  if (!params.length) return raw
  const separator = raw.includes('?') ? '&' : '?'
  return `${raw}${separator}${new URLSearchParams(params).toString()}`
}
function createId(prefix: string): string {
  return globalThis.crypto?.randomUUID?.() ?? `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
}
function restoreSavedUrl(value: string): void {
  try {
    const target = new URL(value)
    url.value = `${target.origin}${target.pathname}${target.hash}`
    queryRows.value = [...target.searchParams.entries()].map(([key, itemValue]) => ({ key, value: itemValue, enabled: true }))
    if (!queryRows.value.length) queryRows.value = [{ key: '', value: '', enabled: true }]
  } catch {
    url.value = value
    queryRows.value = [{ key: '', value: '', enabled: true }]
  }
}
async function loadSavedRequests(): Promise<void> {
  try {
    savedRequests.value = await workbench.commands.execute<string | undefined, SavedApiRequest[]>('api.request.list', selectedModuleId.value, workbench.commandContext)
  } catch (cause) { error.value = cause instanceof Error ? cause.message : t('apiErrorLoadRequests') }
}
async function selectModule(moduleId?: string): Promise<void> {
  selectedModuleId.value = moduleId
  await loadSavedRequests()
}
async function saveModule(): Promise<void> {
  const name = moduleName.value.trim()
  if (!name) return
  try {
    const module = await workbench.commands.execute<ApiModule, ApiModule>('api.module.save', { id: createId('module'), name, createdAt: 0, updatedAt: 0 }, workbench.commandContext)
    modules.value = [module, ...modules.value.filter((item) => item.id !== module.id)]
    moduleName.value = ''
    await selectModule(module.id)
  } catch (cause) { error.value = cause instanceof Error ? cause.message : t('apiErrorSaveModule') }
}
async function deleteModule(module: ApiModule): Promise<void> {
  try {
    await workbench.commands.execute<string, boolean>('api.module.delete', module.id, workbench.commandContext)
    modules.value = modules.value.filter((item) => item.id !== module.id)
    await selectModule(selectedModuleId.value === module.id ? undefined : selectedModuleId.value)
  } catch (cause) { error.value = cause instanceof Error ? cause.message : t('apiErrorDeleteModule') }
}
async function saveRequest(): Promise<void> {
  const name = saveName.value.trim()
  if (!name) { error.value = t('apiErrorRequestNameRequired'); return }
  try {
    const existing = savedRequests.value.find((item) => item.id === currentRequestId.value)
    const saved = await workbench.commands.execute<SavedApiRequest, SavedApiRequest>('api.request.save', {
      id: currentRequestId.value ?? createId('request'),
      name,
      method: method.value,
      url: buildSavedUrl(),
      headers: Object.fromEntries(headerRows.value.filter((row) => row.enabled && row.key.trim()).map((row) => [row.key.trim(), row.value])),
      ...(body.value.trim() ? { body: body.value } : {}),
      ...(selectedModuleId.value ? { moduleId: selectedModuleId.value } : {}),
      createdAt: existing?.createdAt ?? 0,
      updatedAt: Date.now(),
    }, workbench.commandContext)
    currentRequestId.value = saved.id
    saveName.value = saved.name
    savedRequests.value = [saved, ...savedRequests.value.filter((item) => item.id !== saved.id)]
    saveState.value = 'saved'
    window.setTimeout(() => { saveState.value = 'idle' }, 1600)
    error.value = ''
  } catch (cause) { error.value = cause instanceof Error ? cause.message : t('apiErrorSaveRequest') }
}
function loadSavedRequest(item: SavedApiRequest): void {
  currentRequestId.value = item.id
  saveName.value = item.name
  method.value = item.method
  restoreSavedUrl(item.url)
  headerRows.value = Object.entries(item.headers).map(([key, value]) => ({ key, value, enabled: true }))
  if (!headerRows.value.length) headerRows.value = [{ key: '', value: '', enabled: true }]
  body.value = item.body ?? ''
  response.value = undefined
  error.value = ''
}
async function deleteSavedRequest(item: SavedApiRequest): Promise<void> {
  try {
    await workbench.commands.execute<string, boolean>('api.request.delete', item.id, workbench.commandContext)
    savedRequests.value = savedRequests.value.filter((request) => request.id !== item.id)
    if (currentRequestId.value === item.id) { currentRequestId.value = undefined; saveName.value = '' }
  } catch (cause) { error.value = cause instanceof Error ? cause.message : t('apiErrorDeleteRequest') }
}
function clearResponse(): void { response.value = undefined; responseTab.value = 'pretty' }
async function sendRequest(): Promise<void> {
  loading.value = true; error.value = ''; response.value = undefined
  try {
    const request = buildRequest()
    response.value = await workbench.commands.execute<HttpRequest, HttpResponse>('utility.api.send', request, workbench.commandContext)
    const historyUrl = sanitizeHistoryUrl(url.value.trim())
    history.value = [{ method: request.method, url: historyUrl, status: response.value.status, durationMs: response.value.durationMs, at: Date.now() }, ...history.value.filter((item) => item.url !== historyUrl || item.method !== request.method)].slice(0, 10)
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : t('apiErrorRequestFailed')
  } finally { loading.value = false }
}
function formatBody(): void {
  error.value = ''
  try { body.value = JSON.stringify(JSON.parse(body.value) as unknown, null, 2) } catch { error.value = t('apiErrorInvalidJson') }
}
function loadHistory(item: RequestHistory): void { method.value = item.method; url.value = item.url; response.value = undefined; error.value = '' }
function clearHistory(): void { history.value = [] }
function newRequest(): void { currentRequestId.value = undefined; saveName.value = ''; method.value = 'GET'; url.value = ''; queryRows.value = [{ key: '', value: '', enabled: true }]; headerRows.value = [{ key: 'Accept', value: 'application/json', enabled: true }]; body.value = ''; response.value = undefined; error.value = '' }
function sanitizeHistoryUrl(value: string): string {
  try {
    const target = new URL(value)
    target.username = ''; target.password = ''
    for (const key of [...target.searchParams.keys()]) if (/(token|secret|password|passwd|api[-_]?key|authorization)/i.test(key)) target.searchParams.set(key, '•••')
    return target.toString()
  } catch { return value.slice(0, 180) }
}
async function copy(value: string): Promise<void> {
  if (!value) return
  try { await navigator.clipboard.writeText(value); copied.value = true; window.setTimeout(() => { copied.value = false }, 1200) } catch { error.value = t('apiErrorClipboard') }
}
onMounted(async () => {
  try { modules.value = await workbench.commands.execute<undefined, ApiModule[]>('api.module.list', undefined, workbench.commandContext) } catch (cause) { error.value = cause instanceof Error ? cause.message : t('apiErrorLoadModules') }
  await loadSavedRequests()
})
</script>

<template>
  <section class="view api-view">
    <header class="view-header api-header"><div><p class="eyebrow">{{ t('apiKicker') }}</p><h1>{{ t('apiWorkbench') }}</h1><p>{{ t('apiSubtitle') }}</p></div><span class="api-local-badge"><span />{{ t('apiLocalBadge') }}</span></header>
    <div class="api-layout" :class="{ 'api-sidebar-collapsed': apiSidebarCollapsed }">
      <aside class="api-sidebar" :class="{ collapsed: apiSidebarCollapsed }">
        <button class="api-sidebar-toggle icon-button" type="button" :aria-label="apiSidebarCollapsed ? t('apiExpandSidebar') : t('apiCollapseSidebar')" :title="apiSidebarCollapsed ? t('apiExpandSidebar') : t('apiCollapseSidebar')" @click="toggleApiSidebar"><ChevronRight v-if="apiSidebarCollapsed" :size="15" /><ChevronLeft v-else :size="15" /></button>
        <div v-if="!apiSidebarCollapsed" class="api-sidebar-content">
        <button class="primary api-new-button" @click="newRequest"><Plus :size="14" />{{ t('apiNewRequest') }}</button>
        <div class="api-side-section api-modules"><div class="api-side-heading"><span>{{ t('apiModules') }}</span><small>{{ modules.length }}</small></div><div class="api-module-create"><input v-model="moduleName" class="utility-input" :placeholder="t('apiNewModulePlaceholder')" @keydown.enter="saveModule" /><button class="icon-button" :aria-label="t('apiAddModule')" @click="saveModule"><Plus :size="13" /></button></div><button class="api-module-item" :class="{ active: !selectedModuleId }" @click="selectModule()"><span>{{ t('apiAllRequests') }}</span><small>{{ selectedModuleId ? '' : savedRequests.length }}</small></button><div v-for="module in modules" :key="module.id" class="api-module-row"><button class="api-module-item" :class="{ active: selectedModuleId === module.id }" @click="selectModule(module.id)"><span>{{ module.name }}</span><small>{{ selectedModuleId === module.id ? savedRequests.length : '' }}</small></button><button class="icon-button api-module-delete" :aria-label="t('apiDeleteModule', { name: module.name })" @click="deleteModule(module)"><Trash2 :size="12" /></button></div><p v-if="!modules.length" class="api-empty">{{ t('apiModulesEmpty') }}</p></div>
        <div class="api-side-section"><div class="api-side-heading"><span>{{ t('apiEnvironment') }}</span><small>{{ environment }}</small></div><input v-model="environment" class="utility-input" :placeholder="t('apiEnvironmentPlaceholder')" /><textarea v-model="environmentText" class="api-env-editor" spellcheck="false" placeholder="API_BASE_URL=https://api.example.com&#10;TOKEN=…" /><p>{{ t('apiEnvironmentHint') }}</p></div>
        <div class="api-side-section api-saved"><div class="api-side-heading"><span><Send :size="13" />{{ t('apiSavedRequests') }}</span><small>{{ savedRequests.length }}</small></div><div v-for="item in savedRequests" :key="item.id" class="api-saved-item" :class="{ active: currentRequestId === item.id }" role="button" tabindex="0" @click="loadSavedRequest(item)" @keydown.enter="loadSavedRequest(item)"><strong :class="`method-${item.method.toLowerCase()}`">{{ item.method }}</strong><span>{{ item.name }}</span><small>{{ item.url }}</small><button class="icon-button" :aria-label="t('apiDeleteRequest', { name: item.name })" @click.stop="deleteSavedRequest(item)"><Trash2 :size="12" /></button></div><p v-if="!savedRequests.length" class="api-empty">{{ t('apiNoSavedRequests') }}</p></div>
        <div class="api-side-section api-history"><div class="api-side-heading"><span><Clock3 :size="13" />{{ t('apiRecentRequests') }}</span><button v-if="history.length" class="icon-button" :aria-label="t('apiClearHistory')" @click="clearHistory"><Trash2 :size="13" /></button></div><button v-for="item in history" :key="`${item.method}-${item.at}`" class="api-history-item" @click="loadHistory(item)"><strong :class="`method-${item.method.toLowerCase()}`">{{ item.method }}</strong><span>{{ item.url }}</span><small :class="`status-${item.status >= 400 ? 'danger' : 'success'}`">{{ item.status }} · {{ item.durationMs }}ms</small></button><p v-if="!history.length" class="api-empty">{{ t('apiHistoryEmpty') }}</p></div>
        </div>
      </aside>
      <main class="api-main">
        <div class="api-request-bar"><select v-model="method" class="api-method" :aria-label="t('apiHttpMethod')"><option v-for="item in (['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as HttpMethod[])" :key="item">{{ item }}</option></select><input v-model="url" class="utility-input api-url" spellcheck="false" placeholder="https://api.example.com/users" @keydown.ctrl.enter.prevent="sendRequest" @keydown.meta.enter.prevent="sendRequest" /><button class="primary api-send-button" :disabled="loading" @click="sendRequest"><Send :size="14" />{{ loading ? t('apiSending') : t('apiSend') }}</button></div>
        <div class="api-save-bar"><input v-model="saveName" class="utility-input" :placeholder="t('apiRequestNamePlaceholder')" @keydown.enter="saveRequest" /><button class="ghost-button" @click="saveRequest"><Check :size="13" />{{ currentRequestId ? t('apiUpdate') : t('apiSaveRequest') }}</button><span class="api-save-target">{{ selectedModuleId ? modules.find((item) => item.id === selectedModuleId)?.name : t('apiUngrouped') }}</span><span v-if="saveState === 'saved'" class="api-save-feedback"><Check :size="12" />{{ t('apiSaved') }}</span></div>
        <div class="api-tabs" role="tablist"><button v-for="tab in (['params', 'headers', 'body'] as ApiTab[])" :key="tab" :class="{ active: activeTab === tab }" role="tab" :aria-selected="activeTab === tab" @click="activeTab = tab">{{ tab === 'params' ? t('apiParams') : tab === 'headers' ? `${t('apiHeaders')} (${headerRows.filter((row) => row.enabled && row.key).length})` : t('apiBody') }}</button></div>
        <section v-if="activeTab === 'params' || activeTab === 'headers'" class="api-editor-panel"><div class="api-editor-heading"><div><h2>{{ activeTab === 'params' ? t('apiQueryParams') : t('apiHeaders') }}</h2><p>{{ t('apiRowsHint') }}</p></div><button class="ghost-button" @click="addRow(activeTab === 'params' ? queryRows : headerRows)"><Plus :size="13" />{{ t('apiAddRow') }}</button></div><div class="api-kv-head"><span /><span>{{ t('name') }}</span><span>{{ t('value') }}</span><span /></div><div v-for="(row, index) in (activeTab === 'params' ? queryRows : headerRows)" :key="`${activeTab}-${index}`" class="api-kv-row"><input v-model="row.enabled" type="checkbox" :aria-label="t('apiEnableRow')" /><input v-model="row.key" class="utility-input" :placeholder="activeTab === 'params' ? 'userId' : 'Authorization'" /><input v-model="row.value" class="utility-input" :placeholder="activeTab === 'params' ? '1001' : 'Bearer &#123;&#123;TOKEN&#125;&#125;'" /><button class="icon-button" :aria-label="t('apiRemoveRow')" @click="removeRow(activeTab === 'params' ? queryRows : headerRows, index)"><X :size="13" /></button></div></section>
        <section v-else class="api-editor-panel api-body-panel"><div class="api-editor-heading"><div><h2>JSON Body</h2><p>{{ t('apiBodyHint') }}</p></div><button class="ghost-button" @click="formatBody">{{ t('apiFormatJson') }}</button></div><textarea v-model="body" class="api-body-editor" spellcheck="false" placeholder="{&#10;  &quot;name&quot;: &quot;Jensen&quot;&#10;}" /></section>
        <div class="api-request-options"><label>{{ t('timeout') }}<input v-model.number="timeoutMs" class="utility-input timeout-input" type="number" min="1000" max="120000" step="1000" /> ms</label><span class="api-shortcut">Ctrl / ⌘ + Enter {{ t('apiSendShortcut') }}</span></div>
        <section class="api-response-panel"><div class="api-response-heading"><div><span class="section-kicker">{{ t('apiResponse') }}</span><strong v-if="response" :class="`response-status ${responseTone}`">{{ response.status }} {{ response.statusText }}</strong><span v-else class="api-no-response">{{ t('apiWaiting') }}</span></div><div v-if="response" class="api-response-meta"><span>{{ response.durationMs }} ms</span><span>{{ t('apiChars', { count: response.body.length }) }}{{ response.truncated ? ` · ${t('apiTruncated')}` : '' }}</span><button class="icon-button" :aria-label="t('apiCopyResponse')" @click="copy(response.body)"><Check v-if="copied" :size="14" /><Copy v-else :size="14" /></button><button class="icon-button" :aria-label="t('apiClearResponse')" @click="clearResponse"><X :size="14" /></button></div></div><div v-if="response" class="api-response-tabs"><button :class="{ active: responseTab === 'pretty' }" @click="responseTab = 'pretty'">{{ t('apiPretty') }}</button><button :class="{ active: responseTab === 'raw' }" @click="responseTab = 'raw'">{{ t('apiRaw') }}</button><button :class="{ active: responseTab === 'headers' }" @click="responseTab = 'headers'">{{ t('apiHeaders') }} ({{ Object.keys(response.headers).length }})</button></div><pre v-if="response && responseTab === 'pretty'" class="api-response-body">{{ responseBody }}</pre><pre v-else-if="response && responseTab === 'raw'" class="api-response-body">{{ response.body }}</pre><div v-else-if="response && responseTab === 'headers'" class="api-response-headers"><div v-for="(value, key) in response.headers" :key="key"><code>{{ key }}</code><span>{{ value }}</span></div></div><div v-else class="api-response-empty"><div class="api-empty-icon"><Send :size="18" /></div><div><strong>{{ t('apiNoResponse') }}</strong><p>{{ t('apiNoResponseHint') }}</p></div></div></section>
        <p v-if="error" class="api-error">{{ error }}</p>
      </main>
    </div>
  </section>
</template>

<style scoped>
.api-view { min-width: 0; }.api-header { align-items: center; }.api-local-badge { display: inline-flex; align-items: center; gap: 8px; color: #7ee2b8; font-size: 10px; font-weight: 750; letter-spacing: .08em; }.api-local-badge > span { width: 7px; height: 7px; border-radius: 50%; background: #7ee2b8; box-shadow: 0 0 0 4px rgb(126 226 184 / .12); }.api-layout { display: grid; grid-template-columns: 235px minmax(0, 1fr); min-height: 640px; overflow: hidden; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow-sm); }.api-sidebar { padding: 12px 10px; border-right: 1px solid var(--border); background: rgb(0 0 0 / .07); }.api-new-button { width: 100%; justify-content: center; }.api-side-section { margin-top: 24px; }.api-side-heading { display: flex; align-items: center; justify-content: space-between; min-height: 23px; color: var(--muted); font-size: 10px; font-weight: 750; }.api-side-heading span { display: inline-flex; align-items: center; gap: 6px; }.api-side-heading small { color: var(--accent); }.api-env-editor { width: 100%; min-height: 88px; margin-top: 8px; box-sizing: border-box; resize: vertical; padding: 8px; border: 1px solid var(--border); border-radius: 6px; background: #0b0e13; color: #dce4f2; font: 10px/1.5 "Cascadia Code", Consolas, monospace; }.api-side-section > p, .api-empty { color: var(--faint); font-size: 9px; line-height: 1.5; }.api-history { border-top: 1px solid var(--border); }.api-history-item { display: grid; grid-template-columns: 42px minmax(0, 1fr); gap: 3px 7px; width: 100%; padding: 9px 2px; border: 0; border-bottom: 1px solid var(--border); background: transparent; color: var(--muted); text-align: left; cursor: pointer; }.api-history-item:hover { background: var(--surface-2); }.api-history-item strong { font-size: 9px; }.api-history-item span { overflow: hidden; color: var(--foreground); font: 10px "Cascadia Code", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }.api-history-item small { grid-column: 2; font-size: 9px; }.method-get, .method-head, .method-options { color: #7ec8f5; }.method-post { color: #7ee2b8; }.method-put, .method-patch { color: #f0c87a; }.method-delete { color: #ef9aac; }.status-success { color: #7ee2b8; }.status-danger { color: #ef9aac; }.api-main { min-width: 0; padding: 16px; }.api-request-bar { display: flex; gap: 7px; }.api-method { width: 105px; padding: 8px 9px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-2); color: var(--accent); font: 11px "Cascadia Code", Consolas, monospace; font-weight: 750; }.api-url { flex: 1; font-family: "Cascadia Code", Consolas, monospace; }.api-send-button { min-width: 92px; justify-content: center; }.api-tabs, .api-response-tabs { display: flex; gap: 3px; margin-top: 16px; border-bottom: 1px solid var(--border); }.api-tabs button, .api-response-tabs button { padding: 8px 11px; border: 0; border-bottom: 2px solid transparent; background: transparent; color: var(--muted); font-size: 10px; cursor: pointer; }.api-tabs button.active, .api-response-tabs button.active { border-bottom-color: var(--accent); color: var(--accent); }.api-editor-panel { min-height: 190px; padding-top: 16px; }.api-editor-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }.api-editor-heading h2 { margin: 0; color: var(--foreground); font-size: 13px; }.api-editor-heading p { margin: 4px 0 0; color: var(--faint); font-size: 10px; }.api-kv-head, .api-kv-row { display: grid; grid-template-columns: 25px minmax(0, .85fr) minmax(0, 1.35fr) 28px; align-items: center; gap: 7px; }.api-kv-head { margin-top: 18px; color: var(--faint); font-size: 9px; }.api-kv-row { margin-top: 6px; }.api-kv-row input[type='checkbox'] { justify-self: center; accent-color: var(--accent); }.api-kv-row .utility-input { width: 100%; }.api-body-panel { min-height: 255px; }.api-body-editor { width: 100%; min-height: 210px; margin-top: 18px; box-sizing: border-box; resize: vertical; padding: 12px; border: 1px solid var(--border); border-radius: 6px; outline: 0; background: #0b0e13; color: #dce4f2; font: 11px/1.65 "Cascadia Code", Consolas, monospace; }.api-body-editor:focus, .api-env-editor:focus { border-color: var(--accent); outline: 2px solid var(--accent-soft); }.api-request-options { display: flex; align-items: center; justify-content: space-between; margin-top: 14px; color: var(--muted); font-size: 10px; }.api-request-options label { display: inline-flex; align-items: center; gap: 6px; }.timeout-input { width: 90px; }.api-shortcut { color: var(--faint); }.api-response-panel { min-height: 245px; margin-top: 18px; border-top: 1px solid var(--border); }.api-response-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 14px 0 7px; }.section-kicker { margin-right: 10px; }.response-status { font: 12px "Cascadia Code", Consolas, monospace; }.response-status.success { color: #7ee2b8; }.response-status.warning { color: #f0c87a; }.response-status.danger { color: #ef9aac; }.api-no-response { color: var(--faint); font-size: 10px; }.api-response-meta { display: flex; align-items: center; gap: 12px; color: var(--muted); font: 10px "Cascadia Code", Consolas, monospace; }.api-response-body { min-height: 190px; max-height: 360px; overflow: auto; margin: 10px 0 0; padding: 12px; border: 1px solid var(--border); border-radius: 6px; background: #0b0e13; color: #dce4f2; font: 11px/1.65 "Cascadia Code", Consolas, monospace; white-space: pre-wrap; word-break: break-word; }.api-response-headers { max-height: 250px; overflow: auto; margin-top: 10px; border-top: 1px solid var(--border); }.api-response-headers > div { display: grid; grid-template-columns: 170px minmax(0, 1fr); gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--border); font-size: 10px; }.api-response-headers code { color: var(--accent); }.api-response-headers span { overflow-wrap: anywhere; color: var(--muted); }.api-response-empty { display: grid; min-height: 190px; place-items: center; align-content: center; gap: 8px; color: var(--faint); }.api-response-empty p { margin: 0; font-size: 10px; }.api-error { margin: 12px 0 0; color: var(--danger); font-size: 10px; }
.api-module-create, .api-save-bar { display: flex; align-items: center; gap: 6px; }.api-module-create { margin-top: 7px; }.api-module-create .utility-input { min-width: 0; flex: 1; }.api-module-item { display: flex; align-items: center; justify-content: space-between; width: 100%; margin-top: 4px; padding: 7px 7px; border: 0; border-radius: 5px; background: transparent; color: var(--muted); font-size: 10px; text-align: left; cursor: pointer; }.api-module-item:hover, .api-module-item.active { background: var(--surface-2); color: var(--foreground); }.api-module-item.active { box-shadow: inset 2px 0 var(--accent); }.api-module-item small, .api-saved-item small { color: var(--faint); }.api-module-row { display: flex; align-items: center; gap: 2px; }.api-module-row .api-module-item { flex: 1; }.api-module-delete { opacity: 0; }.api-module-row:hover .api-module-delete { opacity: 1; }.api-saved { border-top: 1px solid var(--border); padding-top: 13px; }.api-saved-item { display: grid; grid-template-columns: 37px minmax(0, 1fr) 20px; gap: 3px 6px; width: 100%; margin-top: 4px; padding: 8px 5px; border: 0; border-radius: 5px; background: transparent; color: var(--muted); text-align: left; cursor: pointer; }.api-saved-item:hover, .api-saved-item.active { background: var(--surface-2); }.api-saved-item strong { font-size: 9px; }.api-saved-item span { overflow: hidden; color: var(--foreground); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }.api-saved-item small { grid-column: 1 / 3; overflow: hidden; font: 9px "Cascadia Code", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }.api-saved-item .icon-button { grid-column: 3; grid-row: 1 / 3; }.api-save-bar { margin-top: 9px; }.api-save-bar .utility-input { min-width: 120px; flex: 1; }.api-save-bar > span { color: var(--faint); font-size: 9px; white-space: nowrap; }
.api-layout { grid-template-rows: minmax(0, 1fr); transition: grid-template-columns .18s ease; }.api-sidebar { position: relative; min-width: 0; overflow: hidden auto; }.api-sidebar-toggle { position: absolute; top: 10px; right: 8px; z-index: 1; width: 25px; height: 25px; }.api-sidebar-content { min-width: 0; padding-top: 30px; }.api-sidebar.collapsed { display: flex; align-items: center; justify-content: flex-start; padding: 42px 7px 12px; }.api-sidebar.collapsed .api-sidebar-toggle { top: 10px; right: 50%; transform: translateX(50%); }.api-layout.api-sidebar-collapsed { grid-template-columns: 46px minmax(0, 1fr); }.api-layout:not(.api-sidebar-collapsed) { grid-template-columns: 235px minmax(0, 1fr); }.api-new-button { min-height: 32px; font-size: 10px; }.api-side-section { margin-top: 18px; padding-top: 2px; }.api-side-section + .api-side-section { border-top: 1px solid rgb(255 255 255 / .045); }.api-side-heading { min-height: 26px; letter-spacing: .01em; }.api-env-editor { min-height: 76px; margin-top: 7px; border-color: rgb(255 255 255 / .1); }.api-side-section > p, .api-empty { margin: 7px 3px 0; color: var(--faint); }.api-saved-item { padding-block: 7px; }.api-save-bar { padding: 1px 0; }.api-save-target { padding: 4px 7px; border: 1px solid var(--border); border-radius: 999px; background: var(--surface-2); }.api-save-feedback { display: inline-flex; align-items: center; gap: 4px; color: #7ee2b8 !important; }.api-local-badge { padding: 5px 8px; border: 1px solid rgb(126 226 184 / .18); border-radius: 999px; background: rgb(126 226 184 / .04); }.api-url:focus { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent-soft); }.api-tabs { gap: 4px; margin-top: 14px; }.api-tabs button, .api-response-tabs button { border-radius: 5px 5px 0 0; }.api-tabs button.active, .api-response-tabs button.active { background: var(--accent-soft); font-weight: 700; }.api-editor-panel { padding-top: 14px; }.api-editor-heading p { color: var(--faint); }.api-kv-head { margin-top: 14px; padding: 0 8px; text-transform: uppercase; letter-spacing: .08em; }.api-kv-row { min-height: 32px; margin-top: 5px; }.api-response-panel { min-height: 260px; margin-top: 22px; padding: 0 12px 12px; border: 1px solid var(--border); border-radius: 8px; background: rgb(0 0 0 / .08); }.api-response-heading { min-height: 48px; padding: 10px 0 7px; }.api-response-tabs { margin-top: 4px; }.api-response-empty { min-height: 184px; grid-template-columns: auto minmax(0, 280px); justify-content: center; text-align: left; }.api-empty-icon { display: grid; width: 38px; height: 38px; place-items: center; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-2); color: var(--accent); }.api-response-empty strong { color: var(--muted); font-size: 11px; }.api-response-empty p { max-width: 260px; margin-top: 5px; color: var(--faint); font-size: 10px; line-height: 1.5; }.api-response-body { margin-top: 8px; }
@media (max-width: 800px) { .api-layout { grid-template-columns: 190px minmax(0, 1fr); }.api-layout.api-sidebar-collapsed { grid-template-columns: 46px minmax(0, 1fr); }.api-main { padding: 12px; }.api-request-bar { flex-wrap: wrap; }.api-url { min-width: 100%; order: 3; }.api-send-button { margin-left: auto; } }
@media (max-width: 620px) { .api-header { align-items: flex-start; flex-direction: column; gap: 14px; }.api-layout { grid-template-columns: 1fr; }.api-sidebar { display: none; }.api-kv-head, .api-kv-row { grid-template-columns: 25px minmax(0, .8fr) minmax(0, 1.2fr) 28px; }.api-response-meta span { display: none; } }
</style>
