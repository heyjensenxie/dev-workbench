<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { AlertTriangle, ArrowDownToLine, Check, CheckCircle2, ChevronRight, File, FileImage, FileText, Files, FolderOpen, GripVertical, Image, Layers, Plus, RotateCcw, ScanLine, Settings2, Trash2, UploadCloud, X } from 'lucide-vue-next'
import { open, save } from '@tauri-apps/plugin-dialog'
import { useI18n } from '../i18n'
import { nativeBridge } from '../services/nativeBridge'
import type { FileEngineStatus, ImagesToPdfRequest, PdfCompressionPreset, PdfCompressionResult, PdfMergeRequest, PdfSplitRequest, PdfToWordRequest, WordToPdfRequest } from '@dev-workbench/shared'

type ToolId = 'image-compress' | 'image-convert' | 'image-resize' | 'image-info' | 'pdf-compress' | 'pdf-merge' | 'pdf-split' | 'pdf-images' | 'pdf-render' | 'images-pdf' | 'word-pdf' | 'pdf-word'
type ToolGroup = 'image' | 'pdf' | 'document'
type EntryStatus = 'ready' | 'processing' | 'completed' | 'error'

interface FileEntry {
  id: string
  file: File
  nativePath?: string
  objectUrl?: string
  width?: number
  height?: number
  status: EntryStatus
  output?: ProcessedOutput
}

interface ProcessedOutput {
  blob: Blob
  url: string
  name: string
}

const { locale } = useI18n()
const isZh = computed(() => locale.value === 'zh-CN')
const fileInput = ref<HTMLInputElement>()
const activeTool = ref<ToolId>('image-compress')
const hasExplicitToolSelection = ref(false)
const files = ref<FileEntry[]>([])
const isDragging = ref(false)
const isProcessing = ref(false)
const notice = ref('')
const engineStatus = ref<FileEngineStatus>()
const pdfResult = ref<PdfCompressionResult>()
const preset = ref<'high' | 'balanced' | 'small' | 'custom'>('balanced')
const quality = ref(80)
const outputFormat = ref<'same' | 'jpeg' | 'png' | 'webp'>('same')
const background = ref<'white' | 'black'>('white')
const resizeMode = ref<'scale' | 'width'>('scale')
const scale = ref(50)
const targetWidth = ref(1920)
const lockAspect = ref(true)
const maxInlinePdfBytes = 128 * 1024 * 1024
const splitPages = ref('1')

const copy = computed(() => isZh.value ? {
  kicker: '文件工作台 / 仅本地处理', title: '文件工具', subtitle: '在本机处理图片、PDF 与文档，不上传文件，不把内容送入 AI。', local: '本地处理', noUpload: '内容不会离开此设备', workspace: '工作区', smartStart: '拖入文件后开始', smartHint: '系统会根据文件类型推荐下一步操作。', image: '图片', pdf: 'PDF', document: '文档', compress: '压缩', convert: '格式转换', resize: '调整尺寸', info: '信息', merge: '合并', split: '拆分', extract: '提取图片', render: '页面转图片', toPdf: '图片转 PDF', wordToPdf: 'Word → PDF', pdfToWord: 'PDF → Word', imageCompress: '图片压缩', imageCompressDesc: '批量减小图片体积，原文件保持不变。', imageConvert: '图片格式转换', imageResize: '图片尺寸调整', imageInfo: '图片信息', pdfCompress: 'PDF 压缩', pdfMerge: 'PDF 合并', pdfSplit: 'PDF 拆分', pdfImages: '提取 PDF 图片', pdfRender: 'PDF → 图片', imagesPdf: '图片 → PDF', wordPdf: 'Word → PDF', pdfWord: 'PDF → Word', dropTitle: '将文件拖到这里', dropOr: '或', selectFiles: '选择文件', accepted: '支持单文件或批量选择', chooseFiles: '选择文件', selected: '已选择', clear: '清空', original: '原始大小', output: '输出大小', reduced: '减少', duration: '耗时', settings: '处理设置', preset: '快速预设', high: '高质量', balanced: '平衡', small: '小体积', custom: '自定义', quality: '质量', format: '输出格式', keep: '保留原格式', background: '透明背景处理', white: '白色', black: '黑色', resizeMode: '调整方式', scaleLabel: '缩放比例', widthLabel: '目标宽度', lock: '锁定比例', metadata: '保留元数据', process: '开始处理', processing: '处理中…', download: '下载', processAnother: '继续处理', completed: '处理完成', ready: '准备就绪', unsupported: '当前构建的本地引擎尚未接入此类文件。', engine: '处理引擎', engineHint: '该工作区已预留原生引擎边界；接入后会继续保持本地处理。', details: '处理前分析', imageCount: '张图片', pdfHint: '扫描件可通过下采样显著减小体积；文本型 PDF 会尽量保持清晰。', privacy: '隐私边界', privacyHint: '文件只在当前设备内处理，路径与内容不会写入日志或 AI Context。', detected: '已识别', resolution: '分辨率', filename: '文件名', remove: '移除', error: '处理失败', unsupportedType: '此工具不支持该文件类型。', preview: '预览', originalPreview: '原图', resultPreview: '处理后', noPreview: '暂无预览', smartActions: '推荐操作', openFolder: '打开目录', allLocal: '所有操作均在本地完成', pdfPages: '页', listReady: '个文件已就绪', compare: '前后对比', noEngine: '本地引擎待接入', experimental: '实验性'
} : {
  kicker: 'FILE WORKBENCH / LOCAL ONLY', title: 'File Workbench', subtitle: 'Process images, PDFs and documents on this device. No uploads, no AI context.', local: 'Local processing', noUpload: 'Content never leaves this device', workspace: 'Workspace', smartStart: 'Drop a file to start', smartHint: 'File type detection recommends the next action.', image: 'Image', pdf: 'PDF', document: 'Document', compress: 'Compress', convert: 'Convert', resize: 'Resize', info: 'Info', merge: 'Merge', split: 'Split', extract: 'Extract images', render: 'Render pages', toPdf: 'Images → PDF', wordToPdf: 'Word → PDF', pdfToWord: 'PDF → Word', imageCompress: 'Image Compressor', imageCompressDesc: 'Shrink image files in batches without touching originals.', imageConvert: 'Image Converter', imageResize: 'Image Resize', imageInfo: 'Image Info', pdfCompress: 'PDF Compressor', pdfMerge: 'PDF Merge', pdfSplit: 'PDF Split', pdfImages: 'Extract PDF Images', pdfRender: 'PDF → Images', imagesPdf: 'Images → PDF', wordPdf: 'Word → PDF', pdfWord: 'PDF → Word', dropTitle: 'Drop files here', dropOr: 'or', selectFiles: 'Select files', accepted: 'Single or multiple files supported', chooseFiles: 'Choose files', selected: 'Selected', clear: 'Clear', original: 'Original size', output: 'Output size', reduced: 'Reduced', duration: 'Duration', settings: 'Processing settings', preset: 'Quick preset', high: 'High quality', balanced: 'Balanced', small: 'Small size', custom: 'Custom', quality: 'Quality', format: 'Output format', keep: 'Keep original', background: 'Transparency handling', white: 'White', black: 'Black', resizeMode: 'Resize mode', scaleLabel: 'Scale', widthLabel: 'Target width', lock: 'Lock aspect ratio', metadata: 'Keep metadata', process: 'Process files', processing: 'Processing…', download: 'Download', processAnother: 'Process another', completed: 'Completed', ready: 'Ready', unsupported: 'The local engine for this file type is not connected in this build.', engine: 'Processing engine', engineHint: 'A native engine boundary is reserved here; processing will remain local when connected.', details: 'Preflight analysis', imageCount: 'images', pdfHint: 'Downsampling can significantly reduce scan-heavy PDFs while keeping text PDFs clear.', privacy: 'Privacy boundary', privacyHint: 'Files stay on this device. Paths and content are not written to logs or AI context.', detected: 'Detected', resolution: 'Resolution', filename: 'Filename', remove: 'Remove', error: 'Processing failed', unsupportedType: 'This tool does not support this file type.', preview: 'Preview', originalPreview: 'Original', resultPreview: 'Processed', noPreview: 'No preview yet', smartActions: 'Suggested actions', openFolder: 'Open folder', allLocal: 'All processing stays local', pdfPages: 'pages', listReady: 'files ready', compare: 'Before / after', noEngine: 'Native engine pending', experimental: 'Experimental'
} as const)

const tools = computed(() => [
  { id: 'image-compress' as const, group: 'image' as const, icon: Image, label: copy.value.imageCompress, short: copy.value.compress, priority: 'P0' },
  { id: 'image-convert' as const, group: 'image' as const, icon: FileImage, label: copy.value.imageConvert, short: copy.value.convert, priority: 'P0' },
  { id: 'image-resize' as const, group: 'image' as const, icon: ScanLine, label: copy.value.imageResize, short: copy.value.resize, priority: 'P0' },
  { id: 'image-info' as const, group: 'image' as const, icon: FileText, label: copy.value.imageInfo, short: copy.value.info, priority: 'P1' },
  { id: 'pdf-compress' as const, group: 'pdf' as const, icon: File, label: copy.value.pdfCompress, short: copy.value.compress, priority: 'P0' },
  { id: 'pdf-merge' as const, group: 'pdf' as const, icon: Layers, label: copy.value.pdfMerge, short: copy.value.merge, priority: 'P0' },
  { id: 'pdf-split' as const, group: 'pdf' as const, icon: Files, label: copy.value.pdfSplit, short: copy.value.split, priority: 'P0' },
  { id: 'pdf-images' as const, group: 'pdf' as const, icon: FileImage, label: copy.value.pdfImages, short: copy.value.extract, priority: 'P1' },
  { id: 'pdf-render' as const, group: 'pdf' as const, icon: ScanLine, label: copy.value.pdfRender, short: copy.value.render, priority: 'P1' },
  { id: 'images-pdf' as const, group: 'pdf' as const, icon: FileImage, label: copy.value.imagesPdf, short: copy.value.toPdf, priority: 'P0' },
  { id: 'word-pdf' as const, group: 'document' as const, icon: FileText, label: copy.value.wordPdf, short: copy.value.wordToPdf, priority: 'P1' },
  { id: 'pdf-word' as const, group: 'document' as const, icon: FileText, label: copy.value.pdfWord, short: copy.value.pdfToWord, priority: 'P2' },
])
const activeToolMeta = computed(() => tools.value.find((tool) => tool.id === activeTool.value) ?? tools.value[0])
const activeEngine = computed(() => activeToolMeta.value?.group === 'pdf' ? engineStatus.value?.pdf : activeToolMeta.value?.group === 'document' ? engineStatus.value?.document : undefined)
const compressionEngine = computed(() => engineStatus.value?.pdfCompressor)
const statusEngine = computed(() => activeTool.value === 'pdf-compress' ? compressionEngine.value : activeEngine.value)
const selectedImageFiles = computed(() => files.value.filter((entry) => fileKind(entry.file) === 'image'))
const selectedPdfFiles = computed(() => files.value.filter((entry) => fileKind(entry.file) === 'pdf'))
const selectedDocumentFiles = computed(() => files.value.filter((entry) => fileKind(entry.file) === 'document'))
const canProcessLocally = computed(() => activeTool.value.startsWith('image-') && selectedImageFiles.value.length > 0 && activeTool.value !== 'image-info')
const hasCompleted = computed(() => files.value.some((entry) => entry.status === 'completed'))
const totalOriginalBytes = computed(() => files.value.reduce((sum, entry) => sum + entry.file.size, 0))
const totalOutputBytes = computed(() => files.value.reduce((sum, entry) => sum + (entry.output?.blob.size ?? 0), 0))
const reduction = computed(() => totalOriginalBytes.value && totalOutputBytes.value ? Math.max(0, Math.round((1 - totalOutputBytes.value / totalOriginalBytes.value) * 100)) : 0)
const hasPdfResult = computed(() => Boolean(pdfResult.value))

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let unit = units[0]
  for (let index = 0; index < units.length - 1 && value >= 1024; index += 1) { value /= 1024; unit = units[index + 1] }
  return `${value.toFixed(value >= 10 ? 1 : 2)} ${unit}`
}

function getExtension(name: string): string { return name.split('.').pop()?.toLowerCase() ?? '' }
function fileKind(file: File): ToolGroup | undefined {
  if (file.type.startsWith('image/') || ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif', 'tiff'].includes(getExtension(file.name))) return 'image'
  if (file.type === 'application/pdf' || getExtension(file.name) === 'pdf') return 'pdf'
  if (['doc', 'docx'].includes(getExtension(file.name))) return 'document'
  return undefined
}
function recommendTool(file: File): ToolId | undefined {
  const kind = fileKind(file)
  return kind === 'image' ? 'image-compress' : kind === 'pdf' ? 'pdf-compress' : kind === 'document' ? 'word-pdf' : undefined
}
function selectTool(id: ToolId): void { activeTool.value = id; hasExplicitToolSelection.value = true; notice.value = '' }
function openPicker(): void {
  if (hasTauriRuntime() && (activeTool.value === 'pdf-compress' || activeTool.value === 'pdf-word')) { void selectNativePdf(); return }
  fileInput.value?.click()
}
function handleInput(event: Event): void { const input = event.target as HTMLInputElement; addFiles(Array.from(input.files ?? [])); input.value = '' }
function handleDrop(event: DragEvent): void {
  isDragging.value = false
  const dropped = Array.from(event.dataTransfer?.files ?? [])
  addFiles(dropped)
}
async function addFiles(incoming: File[]): Promise<void> {
  const accepted = incoming.filter((file) => fileKind(file))
  if (!accepted.length) { notice.value = copy.value.unsupportedType; return }
  const firstAccepted = accepted[0]
  const recommendation = firstAccepted ? recommendTool(firstAccepted) : undefined
  // Keep an explicitly selected operation while still recommending a tool
  // when the workbench is in its default state.
  if (recommendation && !hasExplicitToolSelection.value) activeTool.value = recommendation
  for (const file of accepted) {
    const entry: FileEntry = { id: `${file.name}-${file.lastModified}-${Math.random()}`, file, status: 'ready' }
    if (fileKind(file) === 'image') {
      entry.objectUrl = URL.createObjectURL(file)
      try {
        const dimensions = await readImageDimensions(entry.objectUrl)
        entry.width = dimensions.width; entry.height = dimensions.height
      } catch { /* Preview is optional; metadata failure must not block the file list. */ }
    }
    files.value.push(entry)
  }
  notice.value = ''
}
function readImageDimensions(url: string): Promise<{ width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const image = new window.Image()
    image.onload = () => resolve({ width: image.naturalWidth, height: image.naturalHeight })
    image.onerror = () => reject(new Error('Unable to read image'))
    image.src = url
  })
}
function clearFiles(): void {
  for (const entry of files.value) { if (entry.objectUrl) URL.revokeObjectURL(entry.objectUrl); if (entry.output) URL.revokeObjectURL(entry.output.url) }
  files.value = []; pdfResult.value = undefined; notice.value = ''
}
function removeFile(id: string): void {
  const entry = files.value.find((item) => item.id === id)
  if (entry?.objectUrl) URL.revokeObjectURL(entry.objectUrl)
  if (entry?.output) URL.revokeObjectURL(entry.output.url)
  files.value = files.value.filter((item) => item.id !== id)
  if (entry?.nativePath) pdfResult.value = undefined
}
function applyPreset(value: 'high' | 'balanced' | 'small' | 'custom'): void {
  preset.value = value
  if (value !== 'custom') quality.value = value === 'high' ? 90 : value === 'small' ? 65 : 80
}
function outputMime(entry: FileEntry): { mime: string; extension: string } {
  if (outputFormat.value === 'jpeg') return { mime: 'image/jpeg', extension: 'jpg' }
  if (outputFormat.value === 'png') return { mime: 'image/png', extension: 'png' }
  if (outputFormat.value === 'webp') return { mime: 'image/webp', extension: 'webp' }
  return { mime: entry.file.type || 'image/png', extension: getExtension(entry.file.name) || 'png' }
}
function outputName(entry: FileEntry, extension: string): string {
  const base = entry.file.name.replace(/\.[^.]+$/, '')
  const suffix = activeTool.value === 'image-resize' ? '-resized' : activeTool.value === 'image-convert' ? '-converted' : '-compressed'
  return `${base}${suffix}.${extension}`
}
async function processImage(entry: FileEntry): Promise<void> {
  if (!entry.objectUrl) throw new Error('Missing image preview')
  const image = new window.Image()
  image.src = entry.objectUrl
  await new Promise<void>((resolve, reject) => { image.onload = () => resolve(); image.onerror = () => reject(new Error('Unable to decode image')) })
  const ratio = image.naturalHeight / image.naturalWidth || 1
  const width = activeTool.value === 'image-resize' ? (resizeMode.value === 'scale' ? Math.max(1, Math.round(image.naturalWidth * scale.value / 100)) : targetWidth.value) : image.naturalWidth
  const height = activeTool.value === 'image-resize' && resizeMode.value === 'width' && lockAspect.value ? Math.max(1, Math.round(width * ratio)) : activeTool.value === 'image-resize' ? Math.max(1, Math.round(image.naturalHeight * scale.value / 100)) : image.naturalHeight
  const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height
  const context = canvas.getContext('2d'); if (!context) throw new Error('Canvas is unavailable')
  if ((outputFormat.value === 'jpeg' || (outputFormat.value === 'same' && entry.file.type === 'image/jpeg')) && background.value === 'white') { context.fillStyle = '#ffffff'; context.fillRect(0, 0, width, height) }
  if ((outputFormat.value === 'jpeg' || (outputFormat.value === 'same' && entry.file.type === 'image/jpeg')) && background.value === 'black') { context.fillStyle = '#000000'; context.fillRect(0, 0, width, height) }
  context.drawImage(image, 0, 0, width, height)
  const output = outputMime(entry)
  const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob((result) => result ? resolve(result) : reject(new Error('Unable to encode image')), output.mime, output.mime === 'image/png' ? undefined : quality.value / 100))
  entry.output = { blob, url: URL.createObjectURL(blob), name: outputName(entry, output.extension) }
}
function errorMessage(error: unknown, fallback: string): string {
  if (typeof error === 'object' && error !== null && 'message' in error && typeof error.message === 'string') return error.message
  if (error instanceof Error && error.message) return error.message
  return fallback
}
function pdfToWordError(error: unknown): string {
  const message = errorMessage(error, '')
  if (/no extractable text|OCR is required|scanned PDF/i.test(message)) {
    return isZh.value
      ? '该 PDF 是扫描件，没有可提取的文字层。当前本地版本尚未内置 OCR，无法转换为可编辑文字。'
      : 'This PDF is scanned and has no extractable text layer. OCR is not bundled in this local build, so editable text cannot be created.'
  }
  return message || (isZh.value ? 'PDF 转 Word 失败。' : 'PDF to Word failed.')
}
function pdfCompressionHint(): string {
  if (!hasTauriRuntime()) return isZh.value ? 'PDF 压缩需要桌面运行时，请在桌面应用中操作。' : 'PDF compression requires the desktop runtime. Open this tool in the desktop app.'
  if (!compressionEngine.value?.detected) return isZh.value ? '当前构建未包含内置 PDF 引擎，请重新构建应用后再压缩。' : 'This build does not include the built-in PDF engine. Rebuild the application before compressing.'
  return isZh.value ? '请选择输出位置，压缩后的 PDF 会保留在本机。' : 'Choose an output location. The compressed PDF stays on this device.'
}
function pdfWordHint(): string {
  if (!hasTauriRuntime()) return isZh.value ? 'PDF 转 Word 需要桌面运行时，请在桌面应用中操作。' : 'PDF to Word requires the desktop runtime. Open this tool in the desktop app.'
  return isZh.value ? '内置引擎支持文本型 PDF 导出为 DOCX；扫描件和复杂排版需要 OCR 或人工校正。' : 'The built-in engine exports text-based PDFs to DOCX; scanned PDFs and complex layouts need OCR or manual correction.'
}
function wordPdfHint(): string {
  if (!hasTauriRuntime()) return isZh.value ? 'Word 转 PDF 需要桌面运行时，请在桌面应用中操作。' : 'Word to PDF requires the desktop runtime. Open this tool in the desktop app.'
  return isZh.value ? '内置转换器支持 DOCX 文本、段落、字号、粗体、斜体和基础中文；图片、表格与复杂分页暂不保证完全还原。' : 'The built-in converter supports DOCX text, paragraphs, sizes, bold, italic and basic Chinese; images, tables and complex pagination are not guaranteed to match.'
}
async function selectNativePdf(): Promise<FileEntry | undefined> {
  if (!hasTauriRuntime()) { notice.value = pdfCompressionHint(); return }
  const selected = await open({
    title: activeTool.value === 'pdf-word'
      ? (isZh.value ? '选择要转换的 PDF' : 'Choose a PDF to convert')
      : (isZh.value ? '选择要压缩的 PDF' : 'Choose a PDF to compress'),
    multiple: false,
    directory: false,
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  })
  if (typeof selected !== 'string') return
  const existing = files.value.find((entry) => fileKind(entry.file) === 'pdf')
  if (existing) {
    existing.nativePath = selected
    existing.status = 'ready'
    return existing
  }
  const name = selected.split(/[\\/]/).pop() || 'document.pdf'
  const entry: FileEntry = {
    id: `${name}-${Date.now()}-${Math.random()}`,
    file: new globalThis.File([], name, { type: 'application/pdf' }),
    nativePath: selected,
    status: 'ready',
  }
  files.value.push(entry)
  notice.value = ''
  return entry
}
async function compressPdfFile(): Promise<void> {
  if (!hasTauriRuntime() || !nativeBridge.compressPdf) { notice.value = pdfCompressionHint(); return }
  if (!engineStatus.value) await loadEngineStatus()
  if (!compressionEngine.value?.detected) { notice.value = pdfCompressionHint(); return }
  let processingEntry: FileEntry | undefined
  try {
    let entry = files.value.find((item) => fileKind(item.file) === 'pdf')
    if (!entry) return
    const needsNativePath = !entry.nativePath && entry.file.size > maxInlinePdfBytes
    if (needsNativePath) entry = await selectNativePdf()
    if (!entry || (needsNativePath && !entry.nativePath)) return
    const base = entry.file.name.replace(/\.[^.]+$/, '') || 'document'
    const outputPath = await save({
      title: isZh.value ? '保存压缩后的 PDF' : 'Save compressed PDF',
      defaultPath: `${base}-compressed.pdf`,
      filters: [{ name: 'PDF', extensions: ['pdf'] }],
    })
    if (typeof outputPath !== 'string') return
    notice.value = ''
    isProcessing.value = true
    processingEntry = entry
    entry.status = 'processing'
    const inlineBytes = entry.nativePath ? undefined : Array.from(new Uint8Array(await entry.file.arrayBuffer()))
    const request = { inputPath: entry.nativePath ?? entry.file.name, outputPath, preset: preset.value as PdfCompressionPreset, ...(inlineBytes ? { inputBytes: inlineBytes } : {}) }
    pdfResult.value = await nativeBridge.compressPdf(request)
    entry.status = 'completed'
  } catch (error) {
    if (processingEntry) processingEntry.status = 'error'
    notice.value = errorMessage(error, pdfCompressionHint())
  } finally {
    isProcessing.value = false
  }
}
async function mergePdfFiles(): Promise<void> {
  if (!nativeBridge.mergePdfs) { notice.value = copy.value.unsupported; return }
  if (selectedPdfFiles.value.length < 2) { notice.value = isZh.value ? 'PDF 合并至少需要选择两个 PDF 文件。' : 'Select at least two PDF files to merge.'; return }
  const outputPath = await save({
    title: isZh.value ? '保存合并后的 PDF' : 'Save merged PDF',
    defaultPath: 'merged.pdf',
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  })
  if (typeof outputPath !== 'string') return
  const entries = selectedPdfFiles.value
  const usePaths = entries.every((entry) => Boolean(entry.nativePath))
  const inlineBytes = usePaths ? undefined : await Promise.all(entries.map(async (entry) => Array.from(new Uint8Array(await entry.file.arrayBuffer()))))
  const request: PdfMergeRequest = { inputPaths: entries.map((entry) => entry.nativePath ?? entry.file.name), outputPath, ...(inlineBytes ? { inputBytes: inlineBytes } : {}) }
  isProcessing.value = true
  entries.forEach((entry) => { entry.status = 'processing' })
  try {
    pdfResult.value = await nativeBridge.mergePdfs(request)
    entries.forEach((entry) => { entry.status = 'completed' })
    notice.value = ''
  } catch (error) {
    entries.forEach((entry) => { entry.status = 'error' })
    notice.value = errorMessage(error, isZh.value ? 'PDF 合并失败。' : 'Unable to merge the PDF files.')
  } finally { isProcessing.value = false }
}
async function splitPdfFile(): Promise<void> {
  if (!nativeBridge.splitPdf) { notice.value = copy.value.unsupported; return }
  const entry = selectedPdfFiles.value[0]
  if (!entry) { notice.value = isZh.value ? '请选择一个 PDF 文件。' : 'Select a PDF file first.'; return }
  const outputPath = await save({
    title: isZh.value ? '保存拆分后的 PDF' : 'Save split PDF',
    defaultPath: `${entry.file.name.replace(/\.[^.]+$/, '') || 'document'}-pages.pdf`,
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  })
  if (typeof outputPath !== 'string') return
  const request: PdfSplitRequest = {
    inputPath: entry.nativePath ?? entry.file.name,
    outputPath,
    pages: splitPages.value,
    ...(entry.nativePath ? {} : { inputBytes: Array.from(new Uint8Array(await entry.file.arrayBuffer())) }),
  }
  isProcessing.value = true
  entry.status = 'processing'
  try {
    pdfResult.value = await nativeBridge.splitPdf(request)
    entry.status = 'completed'
    notice.value = ''
  } catch (error) {
    entry.status = 'error'
    notice.value = errorMessage(error, isZh.value ? 'PDF 拆分失败，请检查页码范围。' : 'Unable to split the PDF. Check the page range.')
  } finally { isProcessing.value = false }
}
async function pdfToWordFile(): Promise<void> {
  if (!hasTauriRuntime() || !nativeBridge.pdfToWord) { notice.value = pdfWordHint(); return }
  const entry = selectedPdfFiles.value[0]
  if (!entry) { notice.value = isZh.value ? '请选择一个 PDF 文件。' : 'Select a PDF file first.'; return }
  const outputPath = await save({
    title: isZh.value ? '保存 Word 文档' : 'Save Word document',
    defaultPath: `${entry.file.name.replace(/\.[^.]+$/, '') || 'document'}.docx`,
    filters: [{ name: 'Word document', extensions: ['docx'] }],
  })
  if (typeof outputPath !== 'string') return
  let inputBytes: number[] | undefined
  try {
    inputBytes = entry.nativePath ? undefined : Array.from(new Uint8Array(await entry.file.arrayBuffer()))
  } catch {
    notice.value = isZh.value ? '无法读取所选 PDF 内容，请重新选择文件。' : 'The selected PDF could not be read. Please choose it again.'
    return
  }
  if (!entry.nativePath && !inputBytes?.length) {
    notice.value = isZh.value ? '无法读取所选 PDF 内容，请重新选择文件。' : 'The selected PDF could not be read. Please choose it again.'
    return
  }
  const request: PdfToWordRequest = {
    inputPath: entry.nativePath ?? entry.file.name,
    outputPath: ensureOutputExtension(outputPath, 'document.docx'),
    ...(inputBytes ? { inputBytes } : {}),
  }
  isProcessing.value = true
  entry.status = 'processing'
  try {
    pdfResult.value = await nativeBridge.pdfToWord(request)
    entry.status = 'completed'
    notice.value = isZh.value ? 'Word 已导出；已按 PDF 文字位置重排，表格、浮动对象和复杂字体可能仍需校正。' : 'Word exported. Text positions were reconstructed; tables, floating objects and complex fonts may still need correction.'
  } catch (error) {
    entry.status = 'error'
    notice.value = pdfToWordError(error)
  } finally { isProcessing.value = false }
}
async function wordToPdfFile(): Promise<void> {
  if (!hasTauriRuntime() || !nativeBridge.wordToPdf) { notice.value = wordPdfHint(); return }
  const entry = selectedDocumentFiles.value[0]
  if (!entry) { notice.value = isZh.value ? '请选择一个 DOCX 文件。' : 'Select a DOCX file first.'; return }
  if (getExtension(entry.file.name) !== 'docx') {
    notice.value = isZh.value ? '内置 Word 转 PDF 目前只支持 DOCX，不支持旧版 DOC。' : 'The built-in Word to PDF converter currently supports DOCX, not legacy DOC files.'
    return
  }
  const outputPath = await save({
    title: isZh.value ? '保存 PDF 文档' : 'Save PDF document',
    defaultPath: `${entry.file.name.replace(/\.[^.]+$/, '') || 'document'}.pdf`,
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  })
  if (typeof outputPath !== 'string') return
  let inputBytes: number[]
  try {
    inputBytes = Array.from(new Uint8Array(await entry.file.arrayBuffer()))
  } catch {
    notice.value = isZh.value ? '无法读取所选 DOCX 内容，请重新选择文件。' : 'The selected DOCX could not be read. Please choose it again.'
    return
  }
  if (!inputBytes.length) {
    notice.value = isZh.value ? '无法读取所选 DOCX 内容，请重新选择文件。' : 'The selected DOCX could not be read. Please choose it again.'
    return
  }
  const request: WordToPdfRequest = {
    inputPath: entry.file.name,
    outputPath: ensureOutputExtension(outputPath, 'document.pdf'),
    inputBytes,
  }
  isProcessing.value = true
  entry.status = 'processing'
  try {
    pdfResult.value = await nativeBridge.wordToPdf(request)
    entry.status = 'completed'
    notice.value = isZh.value ? 'PDF 已导出；已保留文本和基础样式，图片、表格等复杂结构可能需要校正。' : 'PDF exported. Text and basic styles were preserved; images, tables and complex structures may need correction.'
  } catch (error) {
    entry.status = 'error'
    notice.value = errorMessage(error, isZh.value ? 'Word 转 PDF 失败。' : 'Word to PDF failed.')
  } finally { isProcessing.value = false }
}
async function imagesToPdfFile(): Promise<void> {
  if (!nativeBridge.imagesToPdf) { notice.value = copy.value.unsupported; return }
  if (!selectedImageFiles.value.length) { notice.value = isZh.value ? '请选择至少一张图片。' : 'Select at least one image.'; return }
  const outputPath = await save({
    title: isZh.value ? '保存图片 PDF' : 'Save image PDF',
    defaultPath: 'images.pdf',
    filters: [{ name: 'PDF', extensions: ['pdf'] }],
  })
  if (typeof outputPath !== 'string') return
  const entries = selectedImageFiles.value
  const request: ImagesToPdfRequest = {
    outputPath,
    images: await Promise.all(entries.map(async (entry) => ({ name: entry.file.name, bytes: Array.from(new Uint8Array(await entry.file.arrayBuffer())) }))),
  }
  isProcessing.value = true
  entries.forEach((entry) => { entry.status = 'processing' })
  try {
    pdfResult.value = await nativeBridge.imagesToPdf(request)
    entries.forEach((entry) => { entry.status = 'completed' })
    notice.value = ''
  } catch (error) {
    entries.forEach((entry) => { entry.status = 'error' })
    notice.value = errorMessage(error, isZh.value ? '图片转 PDF 失败。' : 'Unable to create a PDF from the images.')
  } finally { isProcessing.value = false }
}
async function processFiles(): Promise<void> {
  if (!files.value.length) { openPicker(); return }
  if (activeTool.value === 'pdf-compress') { await compressPdfFile(); return }
  if (activeTool.value === 'pdf-merge') { await mergePdfFiles(); return }
  if (activeTool.value === 'pdf-split') { await splitPdfFile(); return }
  if (activeTool.value === 'pdf-word') { await pdfToWordFile(); return }
  if (activeTool.value === 'word-pdf') { await wordToPdfFile(); return }
  if (activeTool.value === 'images-pdf') { await imagesToPdfFile(); return }
  if (!canProcessLocally.value) { notice.value = copy.value.unsupported; return }
  notice.value = ''; isProcessing.value = true
  for (const entry of selectedImageFiles.value) {
    entry.status = 'processing'
    try { await processImage(entry); entry.status = 'completed' } catch { entry.status = 'error' }
  }
  isProcessing.value = false
}
function ensureOutputExtension(path: string, filename: string): string {
  const selectedName = path.split(/[\\/]/).pop() ?? path
  return selectedName.includes('.') ? path : `${path}.${getExtension(filename)}`
}
async function downloadOutput(entry: FileEntry): Promise<void> {
  if (!entry.output) return
  if (!hasTauriRuntime() || !nativeBridge.writeFileBytes) {
    const link = document.createElement('a'); link.href = entry.output.url; link.download = entry.output.name; link.click(); return
  }
  try {
    const selectedPath = await save({
      title: isZh.value ? '保存处理后的文件' : 'Save processed file',
      defaultPath: entry.output.name,
      filters: [{ name: getExtension(entry.output.name).toUpperCase(), extensions: [getExtension(entry.output.name)] }],
    })
    if (typeof selectedPath !== 'string') return
    const path = ensureOutputExtension(selectedPath, entry.output.name)
    const bytes = Array.from(new Uint8Array(await entry.output.blob.arrayBuffer()))
    await nativeBridge.writeFileBytes(path, bytes)
    notice.value = ''
  } catch (error) {
    notice.value = errorMessage(error, isZh.value ? '文件保存失败。' : 'Unable to save the file.')
  }
}
function downloadFirstOutput(): void {
  const first = selectedImageFiles.value[0]
  if (first) downloadOutput(first)
}
function processAnother(): void { clearFiles(); activeTool.value = 'image-compress'; hasExplicitToolSelection.value = false }
function toolGroupLabel(group: ToolGroup): string { return group === 'image' ? copy.value.image : group === 'pdf' ? copy.value.pdf : copy.value.document }
function formatResolution(entry: FileEntry): string { return entry.width && entry.height ? `${entry.width} × ${entry.height}` : '—' }
function outputBytes(entry: FileEntry): number | undefined { return entry.output?.blob.size ?? (pdfResult.value && (pdfResult.value.inputPath === entry.nativePath || (!entry.nativePath && pdfResult.value.inputPath === entry.file.name)) ? pdfResult.value.outputBytes : undefined) }
function selectImageFormat(format: 'same' | 'jpeg' | 'png' | 'webp'): void { outputFormat.value = format; if (format !== 'same') activeTool.value = 'image-convert' }

function hasTauriRuntime(): boolean { return typeof window !== 'undefined' && Reflect.has(window, '__TAURI_INTERNALS__') }
function engineStatusLabel(): string {
  if (!hasTauriRuntime()) return isZh.value ? '桌面运行时可检测' : 'Desktop runtime required'
  if (statusEngine.value?.detected) return statusEngine.value.provider ?? copy.value.engine
  return isZh.value ? '本地引擎未检测到' : 'No local engine detected'
}
function engineStatusMessage(): string {
  if (activeTool.value === 'pdf-compress') return pdfCompressionHint()
  if (activeTool.value === 'pdf-word') return pdfWordHint()
  if (activeTool.value === 'word-pdf') return wordPdfHint()
  if (activeEngine.value?.detected) return copy.value.engineHint
  return copy.value.unsupported
}
async function loadEngineStatus(): Promise<void> {
  if (!hasTauriRuntime() || !nativeBridge.fileEngineStatus) return
  try { engineStatus.value = await nativeBridge.fileEngineStatus() } catch { /* Engine discovery is optional and never blocks the workbench. */ }
}

onBeforeUnmount(clearFiles)
onMounted(() => { void loadEngineStatus() })
</script>

<template>
  <section class="view file-workbench-view">
    <header class="view-header file-header">
      <div>
        <p class="eyebrow">{{ copy.kicker }}</p>
        <h1>{{ copy.title }}</h1>
        <p>{{ copy.subtitle }}</p>
      </div>
      <div class="file-header-meta">
        <span class="local-processing"><span class="local-processing-dot"></span>{{ copy.local }}</span>
        <span class="file-privacy-chip"><Check :size="12" />{{ copy.noUpload }}</span>
      </div>
    </header>

    <div class="file-shell">
      <aside class="file-nav" aria-label="File tools">
        <div class="file-nav-head"><span>{{ copy.workspace }}</span><span class="file-nav-count">{{ tools.length }}</span></div>
        <button v-if="!files.length" class="smart-start" type="button" @click="openPicker"><span class="smart-start-icon"><UploadCloud :size="17" /></span><span><strong>{{ copy.smartStart }}</strong><small>{{ copy.smartHint }}</small></span><ChevronRight :size="14" /></button>
        <div v-for="group in (['image', 'pdf', 'document'] as ToolGroup[])" :key="group" class="file-nav-group">
          <div class="file-nav-group-title"><span>{{ toolGroupLabel(group) }}</span><span class="group-rule"></span></div>
          <button v-for="tool in tools.filter((item) => item.group === group)" :key="tool.id" class="file-tool-item" :class="{ active: activeTool === tool.id }" type="button" @click="selectTool(tool.id)">
            <component :is="tool.icon" :size="15" />
            <span><strong>{{ tool.label }}</strong><small>{{ tool.short }}</small></span>
            <em :class="`priority-${tool.priority.toLowerCase()}`">{{ tool.priority }}</em>
          </button>
        </div>
        <div class="file-nav-footer"><span class="mini-status-dot"></span><span>{{ copy.allLocal }}</span></div>
      </aside>

      <main class="file-main">
        <div class="file-main-head">
          <div class="file-title-line"><span class="file-tool-icon"><component :is="activeToolMeta?.icon" :size="18" /></span><div><h2>{{ activeToolMeta?.label }}</h2><p>{{ activeTool === 'image-compress' ? copy.imageCompressDesc : activeToolMeta?.short }}</p></div></div>
          <div class="file-head-actions"><span v-if="files.length" class="file-count">{{ files.length }} {{ copy.listReady }}</span><button v-if="files.length" class="ghost-button compact-button" type="button" @click="clearFiles"><Trash2 :size="13" />{{ copy.clear }}</button></div>
        </div>

        <div v-if="!files.length" class="file-empty-state" :class="{ dragging: isDragging }" @dragenter.prevent="isDragging = true" @dragover.prevent="isDragging = true" @dragleave.prevent="isDragging = false" @drop.prevent="handleDrop">
          <div class="drop-orbit"><div class="drop-orbit-line"></div><span><UploadCloud :size="25" /></span></div>
          <h3>{{ copy.dropTitle }}</h3>
          <p>{{ copy.dropOr }}</p>
          <button class="primary file-select-button" type="button" @click="openPicker"><FolderOpen :size="15" />{{ copy.selectFiles }}</button>
          <small>{{ copy.accepted }} · JPG, PNG, WebP, PDF, DOCX</small>
        </div>

        <template v-else>
          <div class="file-context-strip"><span class="context-mark"><CheckCircle2 :size="14" /></span><span><strong>{{ copy.selected }} · {{ files.length }} {{ copy.listReady }}</strong><small>{{ files[0]?.file.name }}{{ files.length > 1 ? ` + ${files.length - 1}` : '' }}</small></span><button class="ghost-button compact-button" type="button" @click="openPicker"><Plus :size="13" />{{ copy.chooseFiles }}</button></div>

          <div class="file-content-grid">
            <section class="file-preview-panel">
              <div class="section-heading"><h3><FileImage :size="15" />{{ copy.preview }}</h3><span class="section-meta">{{ selectedImageFiles.length }} {{ copy.imageCount }}</span></div>
              <div class="preview-stage">
                <img v-if="selectedImageFiles[0]?.objectUrl" :src="selectedImageFiles[0].objectUrl" :alt="copy.originalPreview" />
                <div v-else class="preview-placeholder"><File :size="24" /><span>{{ copy.noPreview }}</span></div>
                <span v-if="selectedImageFiles[0]" class="preview-label">{{ copy.originalPreview }}</span>
              </div>
              <div v-if="selectedImageFiles[0]" class="preview-facts"><span><small>{{ copy.format }}</small><strong>{{ getExtension(selectedImageFiles[0].file.name).toUpperCase() }}</strong></span><span><small>{{ copy.resolution }}</small><strong>{{ formatResolution(selectedImageFiles[0]) }}</strong></span><span><small>{{ copy.original }}</small><strong>{{ formatBytes(selectedImageFiles[0].file.size) }}</strong></span></div>
              <div v-if="hasCompleted && selectedImageFiles.length && activeTool !== 'images-pdf'" class="result-preview"><div class="result-preview-head"><span><CheckCircle2 :size="14" />{{ copy.resultPreview }}</span><button class="icon-button" type="button" :title="copy.download" @click="downloadFirstOutput"><ArrowDownToLine :size="14" /></button></div><div class="result-preview-facts"><strong>{{ formatBytes(totalOutputBytes) }}</strong><span>{{ copy.reduced }} {{ reduction }}%</span></div></div>
              <div v-if="hasPdfResult" class="result-preview pdf-result-preview"><div class="result-preview-head"><span><CheckCircle2 :size="14" />{{ copy.resultPreview }}</span></div><div class="result-preview-facts"><strong>{{ formatBytes(pdfResult!.outputBytes) }}</strong><span>{{ copy.reduced }} {{ Math.max(0, Math.round((1 - pdfResult!.outputBytes / pdfResult!.inputBytes) * 100)) }}%</span><span>{{ copy.duration }} {{ pdfResult!.durationMs }} ms</span></div><code class="pdf-output-path">{{ pdfResult!.outputPath }}</code></div>
            </section>

            <section class="file-settings-panel">
              <div class="section-heading"><h3><Settings2 :size="15" />{{ copy.settings }}</h3><span v-if="activeToolMeta?.priority === 'P2'" class="experimental-tag">{{ copy.experimental }}</span></div>
              <template v-if="canProcessLocally">
                <div v-if="activeTool === 'image-compress'" class="setting-block"><label class="setting-label">{{ copy.preset }}</label><div class="preset-grid"><button v-for="item in ([['high', copy.high, 90], ['balanced', copy.balanced, 80], ['small', copy.small, 65], ['custom', copy.custom, null]] as const)" :key="item[0]" :class="{ active: preset === item[0] }" type="button" @click="applyPreset(item[0])"><strong>{{ item[1] }}</strong><small>{{ item[2] ? item[2] : '—' }}</small></button></div></div>
                <div v-if="activeTool === 'image-compress' || activeTool === 'image-convert'" class="setting-block"><div class="setting-label-row"><label class="setting-label">{{ copy.quality }}</label><output>{{ quality }}</output></div><input v-model.number="quality" class="range-input" type="range" min="10" max="100" step="1" @input="preset = 'custom'" /><div class="range-scale"><span>10</span><span>50</span><span>100</span></div></div>
                <div v-if="activeTool === 'image-convert' || activeTool === 'image-compress'" class="setting-block"><label class="setting-label">{{ copy.format }}</label><div class="format-row"><button v-for="item in ([['same', copy.keep], ['jpeg', 'JPEG'], ['png', 'PNG'], ['webp', 'WebP']] as const)" :key="item[0]" :class="{ active: outputFormat === item[0] }" type="button" @click="selectImageFormat(item[0])">{{ item[1] }}</button></div><p v-if="outputFormat === 'jpeg'" class="setting-hint"><AlertTriangle :size="13" />{{ copy.background }}: {{ background === 'white' ? copy.white : copy.black }}</p></div>
                <div v-if="activeTool === 'image-resize'" class="setting-block"><label class="setting-label">{{ copy.resizeMode }}</label><div class="format-row"><button :class="{ active: resizeMode === 'scale' }" type="button" @click="resizeMode = 'scale'">{{ copy.scaleLabel }}</button><button :class="{ active: resizeMode === 'width' }" type="button" @click="resizeMode = 'width'">{{ copy.widthLabel }}</button></div><div class="setting-input-row"><input v-if="resizeMode === 'scale'" v-model.number="scale" class="file-number-input" type="number" min="1" max="200" /><span v-if="resizeMode === 'scale'">%</span><input v-else v-model.number="targetWidth" class="file-number-input" type="number" min="1" max="10000" /><span v-if="resizeMode === 'width'">px</span><label class="check-control"><input v-model="lockAspect" type="checkbox" />{{ copy.lock }}</label></div></div>
                <div class="privacy-note"><span><Check :size="13" /></span><p><strong>{{ copy.privacy }}</strong>{{ copy.privacyHint }}</p></div>
                <button class="primary process-button" type="button" :disabled="isProcessing || !selectedImageFiles.length" @click="processFiles">{{ isProcessing ? copy.processing : copy.process }}<ArrowDownToLine :size="14" /></button>
              </template>
              <template v-else-if="activeTool === 'image-info'">
                <div class="image-info-state"><span class="engine-icon"><FileImage :size="20" /></span><strong>{{ copy.details }}</strong><p>{{ copy.privacyHint }}</p><div v-if="selectedImageFiles[0]" class="info-grid"><span><small>{{ copy.format }}</small><strong>{{ getExtension(selectedImageFiles[0].file.name).toUpperCase() }}</strong></span><span><small>{{ copy.resolution }}</small><strong>{{ formatResolution(selectedImageFiles[0]) }}</strong></span><span><small>{{ copy.original }}</small><strong>{{ formatBytes(selectedImageFiles[0].file.size) }}</strong></span><span><small>{{ copy.metadata }}</small><strong>EXIF local</strong></span></div></div>
                <div class="privacy-note"><span><Check :size="13" /></span><p><strong>{{ copy.privacy }}</strong>{{ copy.privacyHint }}</p></div>
              </template>
              <template v-else>
                <div class="engine-state"><span class="engine-icon"><Settings2 :size="20" /></span><strong>{{ activeTool === 'pdf-word' ? (isZh ? '内置 PDF 文本引擎 · DOCX' : 'Built-in PDF text engine · DOCX') : activeTool === 'word-pdf' ? (isZh ? '内置 DOCX 文本引擎 · PDF' : 'Built-in DOCX text engine · PDF') : activeTool === 'pdf-compress' ? (compressionEngine?.detected ? `${compressionEngine.provider} · ${copy.engine}` : engineStatusLabel()) : (activeEngine?.detected ? `${activeEngine.provider} · ${copy.engine}` : engineStatusLabel()) }}</strong><p>{{ notice || engineStatusMessage() }}</p><div v-if="activeTool === 'pdf-split'" class="operation-input"><label :for="'split-pages-input'">{{ isZh ? '页码范围' : 'Page ranges' }}</label><input id="split-pages-input" v-model="splitPages" type="text" placeholder="1-5, 8, 10-12" /><small>{{ isZh ? '例如：1-5, 8, 10-12' : 'Example: 1-5, 8, 10-12' }}</small></div><div class="engine-line"><span>{{ copy.engine }}</span><em>{{ activeTool === 'pdf-word' || activeTool === 'word-pdf' ? (isZh ? '内置，无需额外安装' : 'Built-in, no install required') : activeTool === 'pdf-compress' ? (compressionEngine?.detected ? compressionEngine.version || compressionEngine.provider : engineStatusLabel()) : (activeEngine?.detected ? activeEngine.version || activeEngine.provider : engineStatusLabel()) }}</em></div></div>
                <div class="analysis-card"><div class="analysis-card-title"><ScanLine :size="14" />{{ copy.details }}</div><p v-if="activeTool?.startsWith('pdf')">{{ copy.pdfHint }}</p><p v-else>{{ copy.engineHint }}</p><div class="analysis-tags"><span>{{ activeToolMeta?.priority }}</span><span>{{ toolGroupLabel(activeToolMeta?.group ?? 'document') }}</span><span>{{ copy.local }}</span></div></div>
                <button class="ghost-button process-button" type="button" :disabled="isProcessing" @click="processFiles"><RotateCcw :size="14" />{{ isProcessing ? copy.processing : copy.process }}</button>
              </template>
            </section>
          </div>

          <section class="file-list-panel"><div class="section-heading"><h3><Files :size="15" />{{ copy.selected }}<span class="count">{{ files.length }}</span></h3><button class="ghost-button compact-button" type="button" @click="clearFiles"><X :size="13" />{{ copy.clear }}</button></div><div class="file-list-table"><div class="file-list-head"><span></span><span>{{ copy.filename }}</span><span>{{ copy.format }}</span><span>{{ copy.resolution }}</span><span>{{ copy.original }}</span><span>{{ copy.output }}</span><span>{{ copy.ready }}</span><span></span></div><div v-for="entry in files" :key="entry.id" class="file-list-row"><GripVertical :size="14" class="drag-handle" /><span class="file-name-cell"><span class="file-type-glyph"><Image v-if="fileKind(entry.file) === 'image'" :size="13" /><File v-else-if="fileKind(entry.file) === 'pdf'" :size="13" /><FileText v-else :size="13" /></span><strong>{{ entry.file.name }}</strong></span><span class="file-format">{{ getExtension(entry.file.name).toUpperCase() }}</span><span class="file-resolution">{{ formatResolution(entry) }}</span><span>{{ formatBytes(entry.file.size) }}</span><span>{{ outputBytes(entry) !== undefined ? formatBytes(outputBytes(entry)!) : '—' }}</span><span :class="`entry-status status-${entry.status}`"><CheckCircle2 v-if="entry.status === 'completed'" :size="12" /><span v-else-if="entry.status === 'processing'" class="status-spinner"></span><AlertTriangle v-else-if="entry.status === 'error'" :size="12" />{{ entry.status === 'completed' ? copy.completed : entry.status === 'processing' ? copy.processing : entry.status === 'error' ? copy.error : copy.ready }}</span><span class="row-actions"><button v-if="entry.output" class="icon-button" type="button" :title="copy.download" @click="downloadOutput(entry)"><ArrowDownToLine :size="13" /></button><button class="icon-button" type="button" :title="copy.remove" @click="removeFile(entry.id)"><X :size="13" /></button></span></div></div></section>
          <div v-if="notice" class="file-notice"><AlertTriangle :size="14" />{{ notice }}<button class="icon-button" type="button" @click="notice = ''"><X :size="13" /></button></div>
          <div v-if="hasCompleted && selectedImageFiles.length" class="file-result-bar"><span class="result-mark"><Check :size="14" /></span><span><strong>{{ copy.completed }}</strong><small>{{ copy.original }} {{ formatBytes(totalOriginalBytes) }} → {{ copy.output }} {{ formatBytes(totalOutputBytes) }} · {{ copy.reduced }} {{ reduction }}%</small></span><button class="primary compact-button" type="button" @click="processAnother">{{ copy.processAnother }}</button></div>
        </template>
      </main>
    </div>
    <input ref="fileInput" class="visually-hidden" aria-hidden="true" tabindex="-1" type="file" multiple accept="image/*,.pdf,.doc,.docx" @change="handleInput" />
  </section>
</template>

<style scoped>
.file-workbench-view { width: min(100%, 1320px); padding-top: 24px; }
.file-header { align-items: center; margin-bottom: 20px; }
.file-header h1 { display: flex; align-items: baseline; gap: 10px; }
.file-header-meta { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; justify-content: flex-end; }
.local-processing { display: inline-flex; align-items: center; gap: 8px; color: #72d5ab; font-size: 10px; font-weight: 750; letter-spacing: .05em; text-transform: uppercase; }
.local-processing-dot, .mini-status-dot { width: 6px; height: 6px; border-radius: 50%; background: #72d5ab; box-shadow: 0 0 0 3px rgb(114 213 171 / .12); }
.file-privacy-chip { display: inline-flex; align-items: center; gap: 6px; padding: 6px 8px; border: 1px solid var(--border); border-radius: 6px; color: var(--muted); font-size: 10px; }
.file-privacy-chip svg { color: var(--success); }
.file-shell { display: grid; grid-template-columns: 234px minmax(0, 1fr); min-height: 670px; overflow: hidden; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow-sm); }
.file-nav { display: flex; min-width: 0; flex-direction: column; padding: 13px 9px 10px; border-right: 1px solid var(--border); background: rgb(0 0 0 / .07); }
.file-nav-head, .file-nav-group-title, .file-head-actions, .file-context-strip, .section-heading, .file-main-head, .setting-label-row, .result-preview-head, .file-result-bar, .file-nav-footer { display: flex; align-items: center; }
.file-nav-head { justify-content: space-between; padding: 3px 9px 12px; color: var(--faint); font-size: 9px; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
.file-nav-count { min-width: 20px; padding: 2px 5px; border-radius: 4px; background: var(--surface-2); color: var(--muted); font-size: 9px; text-align: center; }
.smart-start { display: flex; align-items: center; gap: 8px; width: 100%; margin: 0 0 13px; padding: 9px; border: 1px solid var(--accent-line); border-radius: 7px; background: var(--accent-soft); color: var(--foreground); text-align: left; }
.smart-start:hover { border-color: var(--accent); background: rgb(147 136 245 / .18); }
.smart-start-icon { display: grid; flex: 0 0 auto; place-items: center; width: 29px; height: 29px; border-radius: 6px; background: var(--surface); color: var(--accent); }
.smart-start > span:nth-child(2) { display: grid; min-width: 0; gap: 3px; }
.smart-start strong { font-size: 10px; font-weight: 700; }
.smart-start small { overflow: hidden; color: var(--muted); font-size: 9px; line-height: 1.3; text-overflow: ellipsis; white-space: nowrap; }
.smart-start > svg { margin-left: auto; color: var(--muted); }
.file-nav-group { display: grid; gap: 2px; margin-bottom: 14px; }
.file-nav-group-title { gap: 7px; padding: 3px 9px 6px; color: var(--faint); font-size: 9px; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
.group-rule { flex: 1; height: 1px; background: var(--border); opacity: .75; }
.file-tool-item { position: relative; display: flex; align-items: center; gap: 8px; width: 100%; min-height: 35px; padding: 5px 8px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); text-align: left; }
.file-tool-item:hover { background: var(--surface-2); color: var(--foreground); }
.file-tool-item.active { background: var(--accent-soft); color: var(--foreground); }
.file-tool-item.active::before { position: absolute; top: 9px; bottom: 9px; left: -9px; width: 2px; border-radius: 0 2px 2px 0; background: var(--accent-line); content: ""; }
.file-tool-item > svg { flex: 0 0 auto; color: var(--faint); }
.file-tool-item.active > svg { color: var(--accent); }
.file-tool-item > span { display: grid; min-width: 0; gap: 2px; }
.file-tool-item strong { overflow: hidden; color: inherit; font-size: 10px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
.file-tool-item small { color: var(--faint); font-size: 9px; }
.file-tool-item em { margin-left: auto; padding: 2px 4px; border-radius: 3px; color: var(--faint); font: 700 8px "Cascadia Code", monospace; font-style: normal; }
.file-tool-item em.priority-p0 { color: var(--accent); background: var(--accent-soft); }.file-tool-item em.priority-p2 { color: var(--warning); background: var(--warning-soft); }
.file-nav-footer { gap: 8px; margin-top: auto; padding: 11px 9px 4px; border-top: 1px solid var(--border); color: var(--faint); font-size: 9px; }
.file-main { min-width: 0; padding: 0 18px 20px; }
.file-main-head { justify-content: space-between; gap: 12px; min-height: 76px; border-bottom: 1px solid var(--border); }
.file-title-line { display: flex; align-items: center; gap: 10px; min-width: 0; }
.file-tool-icon { display: grid; flex: 0 0 auto; place-items: center; width: 33px; height: 33px; border: 1px solid var(--accent-line); border-radius: 7px; background: var(--accent-soft); color: var(--accent); }
.file-title-line h2 { margin: 0; color: var(--foreground); font-size: 16px; font-weight: 650; letter-spacing: -.025em; }.file-title-line p { margin: 4px 0 0; color: var(--muted); font-size: 10px; }
.file-head-actions { gap: 7px; }.file-count, .section-meta { color: var(--muted); font-size: 10px; }.compact-button { min-height: 28px; padding-inline: 8px; font-size: 10px; }
.file-empty-state { display: flex; min-height: 540px; flex-direction: column; align-items: center; justify-content: center; gap: 8px; border-bottom: 1px solid var(--border); color: var(--muted); transition: background .16s, border-color .16s; }
.file-empty-state.dragging { background: var(--accent-soft); border-color: var(--accent-line); }.file-empty-state h3 { margin: 5px 0 0; color: var(--foreground); font-size: 15px; font-weight: 650; }.file-empty-state p { margin: 0; color: var(--faint); font-size: 11px; }.file-empty-state small { margin-top: 5px; color: var(--faint); font-size: 9px; }
.drop-orbit { position: relative; display: grid; place-items: center; width: 82px; height: 82px; border: 1px solid var(--accent-line); border-radius: 50%; background: radial-gradient(circle, var(--accent-soft) 0 34%, transparent 35%); color: var(--accent); }.drop-orbit::before, .drop-orbit::after { position: absolute; border: 1px solid var(--accent-line); border-radius: 50%; content: ""; opacity: .36; }.drop-orbit::before { inset: 9px; }.drop-orbit::after { inset: 19px; opacity: .22; }.drop-orbit-line { position: absolute; top: -4px; right: 12px; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 4px var(--accent-soft); }.drop-orbit span { position: relative; z-index: 1; display: grid; place-items: center; width: 38px; height: 38px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); }
.file-select-button { margin-top: 5px; }.file-context-strip { gap: 9px; min-height: 62px; margin-top: 14px; padding: 9px 11px; border: 1px solid var(--border); border-radius: 7px; background: var(--surface-2); }.file-context-strip > span:nth-child(2) { display: grid; min-width: 0; gap: 2px; }.file-context-strip strong { color: var(--foreground); font-size: 10px; }.file-context-strip small { overflow: hidden; color: var(--muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }.file-context-strip .compact-button { margin-left: auto; }.context-mark, .result-mark { display: grid; flex: 0 0 auto; place-items: center; width: 25px; height: 25px; border-radius: 6px; background: var(--success-soft); color: var(--success); }
.file-content-grid { display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(300px, .9fr); gap: 14px; margin-top: 14px; }.file-preview-panel, .file-settings-panel, .file-list-panel { min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); }.section-heading { justify-content: space-between; gap: 10px; margin-bottom: 12px; }.section-heading h3 { display: flex; align-items: center; gap: 7px; margin: 0; color: var(--foreground); font-size: 11px; font-weight: 700; }.section-heading h3 svg { color: var(--muted); }.section-heading .count { margin-left: 2px; }.preview-stage { position: relative; display: grid; min-height: 252px; place-items: center; overflow: hidden; border: 1px solid var(--border); border-radius: 6px; background: #0d1016; background-image: linear-gradient(45deg, rgb(255 255 255 / .025) 25%, transparent 25%), linear-gradient(-45deg, rgb(255 255 255 / .025) 25%, transparent 25%), linear-gradient(45deg, transparent 75%, rgb(255 255 255 / .025) 75%), linear-gradient(-45deg, transparent 75%, rgb(255 255 255 / .025) 75%); background-position: 0 0, 0 8px, 8px -8px, -8px 0; background-size: 16px 16px; }.preview-stage img { display: block; max-width: 100%; max-height: 250px; object-fit: contain; }.preview-label { position: absolute; top: 8px; left: 8px; padding: 3px 5px; border-radius: 4px; background: rgb(0 0 0 / .55); color: #cfd4e1; font-size: 9px; }.preview-placeholder { display: grid; place-items: center; gap: 7px; color: var(--faint); font-size: 10px; }.preview-facts, .result-preview-facts { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1px; margin-top: 10px; border: 1px solid var(--border); background: var(--border); }.preview-facts span, .result-preview-facts > * { display: grid; gap: 4px; padding: 8px; background: var(--surface-2); }.preview-facts small { color: var(--muted); font-size: 9px; }.preview-facts strong { overflow: hidden; color: var(--foreground); font: 10px "Cascadia Code", monospace; text-overflow: ellipsis; white-space: nowrap; }.result-preview { margin-top: 10px; padding: 9px; border: 1px solid rgb(81 195 148 / .3); border-radius: 6px; background: var(--success-soft); }.result-preview-head { justify-content: space-between; color: var(--success); font-size: 10px; font-weight: 700; }.result-preview-head > span { display: inline-flex; align-items: center; gap: 5px; }.result-preview .icon-button { color: var(--success); }.result-preview-facts { margin-top: 8px; border: 0; background: transparent; }.result-preview-facts > * { padding: 0; background: transparent; }.result-preview-facts strong { color: var(--foreground); font: 12px "Cascadia Code", monospace; }.result-preview-facts span { color: var(--success); font-size: 9px; }
.pdf-output-path { display: block; margin-top: 9px; overflow: hidden; color: var(--muted); font: 9px/1.4 "Cascadia Code", monospace; text-overflow: ellipsis; white-space: nowrap; }
.file-settings-panel { display: flex; flex-direction: column; }.setting-block { padding: 13px 0; border-top: 1px solid var(--border); }.setting-label { display: block; margin-bottom: 8px; color: var(--muted); font-size: 10px; font-weight: 700; }.setting-label-row { justify-content: space-between; }.setting-label-row .setting-label { margin-bottom: 0; }.setting-label-row output { color: var(--accent); font: 12px "Cascadia Code", monospace; }.preset-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; }.preset-grid button, .format-row button { min-height: 34px; padding: 4px 6px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); color: var(--muted); font-size: 9px; }.preset-grid button { display: grid; gap: 3px; }.preset-grid button:hover, .format-row button:hover { border-color: var(--border-strong); color: var(--foreground); }.preset-grid button.active, .format-row button.active { border-color: var(--accent-line); background: var(--accent-soft); color: var(--accent); }.preset-grid strong { font-size: 9px; }.preset-grid small { color: var(--faint); font: 9px "Cascadia Code", monospace; }.range-input { width: 100%; accent-color: var(--accent); }.range-scale { display: flex; justify-content: space-between; color: var(--faint); font: 9px "Cascadia Code", monospace; }.format-row { display: flex; gap: 5px; flex-wrap: wrap; }.format-row button { min-width: 58px; }.setting-hint { display: flex; align-items: center; gap: 5px; margin: 8px 0 0; color: var(--warning); font-size: 9px; }.setting-input-row { display: flex; align-items: center; gap: 6px; }.file-number-input { width: 84px; min-height: 31px; padding: 7px 8px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); color: var(--foreground); font-size: 11px; }.file-number-input:focus { border-color: var(--accent-line); outline: 2px solid var(--accent-soft); }.setting-input-row > span { color: var(--muted); font-size: 10px; }.check-control { display: inline-flex; align-items: center; gap: 6px; color: var(--muted); font-size: 10px; }.setting-input-row .check-control { margin-left: auto; }.check-control input { accent-color: var(--accent); }.setting-check { margin: 0 0 13px; }.check-note { padding: 2px 4px; border-radius: 3px; background: var(--surface-2); color: var(--faint); font: 9px "Cascadia Code", monospace; }.privacy-note { display: flex; gap: 8px; margin-top: auto; padding: 10px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-2); }.privacy-note > span { display: grid; flex: 0 0 auto; place-items: center; width: 22px; height: 22px; border-radius: 5px; background: var(--success-soft); color: var(--success); }.privacy-note p { margin: 0; color: var(--muted); font-size: 9px; line-height: 1.4; }.privacy-note strong { display: block; margin-bottom: 2px; color: var(--foreground); font-size: 10px; }.process-button { width: 100%; margin-top: 12px; }.engine-state { display: grid; place-items: center; gap: 7px; min-height: 248px; padding: 18px; border-top: 1px solid var(--border); text-align: center; }.engine-icon { display: grid; place-items: center; width: 40px; height: 40px; border-radius: 9px; background: var(--warning-soft); color: var(--warning); }.engine-state strong { color: var(--foreground); font-size: 12px; }.engine-state p { max-width: 270px; margin: 0; color: var(--muted); font-size: 10px; line-height: 1.5; }.operation-input { display: grid; width: 100%; gap: 4px; margin-top: 3px; text-align: left; }.operation-input label { color: var(--muted); font-size: 10px; font-weight: 700; }.operation-input input { min-height: 32px; padding: 7px 8px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); color: var(--foreground); font: 11px "Cascadia Code", monospace; }.operation-input input:focus { border-color: var(--accent-line); outline: 2px solid var(--accent-soft); }.operation-input small { color: var(--faint); font-size: 9px; }.engine-line { display: flex; justify-content: space-between; width: 100%; margin-top: 5px; padding-top: 9px; border-top: 1px solid var(--border); color: var(--faint); font-size: 9px; }.engine-line em { color: var(--warning); font-style: normal; }.analysis-card { margin-top: auto; padding: 10px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-2); }.analysis-card-title { display: flex; align-items: center; gap: 6px; color: var(--foreground); font-size: 10px; font-weight: 700; }.analysis-card-title svg { color: var(--accent); }.analysis-card p { margin: 7px 0 9px; color: var(--muted); font-size: 9px; line-height: 1.45; }.analysis-tags { display: flex; gap: 4px; flex-wrap: wrap; }.analysis-tags span, .experimental-tag { padding: 3px 5px; border-radius: 4px; background: var(--surface-3); color: var(--faint); font: 9px "Cascadia Code", monospace; }.experimental-tag { color: var(--warning); background: var(--warning-soft); font-family: inherit; }
.file-list-panel { margin-top: 14px; }.file-list-table { overflow-x: auto; }.file-list-head, .file-list-row { display: grid; grid-template-columns: 20px minmax(165px, 1.5fr) 48px minmax(84px, .85fr) minmax(72px, .75fr) minmax(72px, .75fr) minmax(84px, .85fr) 55px; align-items: center; gap: 7px; min-width: 700px; }.file-list-head { min-height: 28px; color: var(--faint); font-size: 8px; font-weight: 800; letter-spacing: .06em; text-transform: uppercase; }.file-list-row { min-height: 46px; border-top: 1px solid var(--border); color: var(--foreground); font-size: 10px; }.drag-handle { color: var(--faint); cursor: grab; }.file-name-cell { display: flex; min-width: 0; align-items: center; gap: 7px; }.file-name-cell strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.file-type-glyph { display: grid; flex: 0 0 auto; place-items: center; width: 24px; height: 24px; border-radius: 5px; background: var(--accent-soft); color: var(--accent); }.file-format, .file-resolution { color: var(--muted); font: 9px "Cascadia Code", monospace; }.entry-status { display: inline-flex; align-items: center; gap: 5px; color: var(--muted); font-size: 9px; }.entry-status.status-completed { color: var(--success); }.entry-status.status-error { color: var(--danger); }.status-spinner { width: 11px; height: 11px; border: 2px solid var(--border-strong); border-top-color: var(--accent); border-radius: 50%; animation: file-spin .8s linear infinite; }.row-actions { display: flex; justify-content: flex-end; gap: 2px; }.row-actions .icon-button { width: 24px; height: 24px; }.file-notice { display: flex; align-items: center; gap: 7px; margin-top: 12px; padding: 9px 10px; border: 1px solid var(--warning); border-radius: 6px; background: var(--warning-soft); color: var(--warning); font-size: 10px; }.file-notice .icon-button { margin-left: auto; color: inherit; }.file-result-bar { gap: 9px; margin-top: 12px; padding: 10px; border: 1px solid rgb(81 195 148 / .35); border-radius: 7px; background: var(--success-soft); }.file-result-bar > span:nth-child(2) { display: grid; gap: 3px; }.file-result-bar strong { color: var(--foreground); font-size: 10px; }.file-result-bar small { color: var(--muted); font-size: 9px; }.file-result-bar .primary { margin-left: auto; }
.visually-hidden { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
@keyframes file-spin { to { transform: rotate(360deg); } }
.image-info-state { display: grid; place-items: center; gap: 7px; min-height: 248px; padding: 18px; border-top: 1px solid var(--border); text-align: center; }.image-info-state > strong { color: var(--foreground); font-size: 12px; }.image-info-state > p { max-width: 270px; margin: 0; color: var(--muted); font-size: 10px; line-height: 1.5; }.info-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); width: 100%; gap: 1px; margin-top: 5px; border: 1px solid var(--border); background: var(--border); text-align: left; }.info-grid span { display: grid; gap: 3px; padding: 8px; background: var(--surface-2); }.info-grid small { color: var(--muted); font-size: 9px; }.info-grid strong { overflow: hidden; color: var(--foreground); font: 10px "Cascadia Code", monospace; text-overflow: ellipsis; white-space: nowrap; }
@media (max-width: 1050px) { .file-content-grid { grid-template-columns: 1fr; }.file-settings-panel { min-height: auto; }.privacy-note { margin-top: 10px; } }
@media (max-width: 820px) { .file-shell { grid-template-columns: 1fr; }.file-nav { display: none; }.file-main { padding-inline: 12px; }.file-header { align-items: flex-start; flex-direction: column; }.file-header-meta { justify-content: flex-start; }.file-main-head { align-items: flex-start; flex-direction: column; justify-content: center; padding: 14px 0; }.file-head-actions { width: 100%; }.file-content-grid { grid-template-columns: 1fr; } }
</style>
