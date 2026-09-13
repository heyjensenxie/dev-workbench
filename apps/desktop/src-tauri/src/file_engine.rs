use flate2::read::DeflateDecoder;
use image::{GenericImageView, imageops::FilterType};
use lopdf::{
    Dictionary, Document, LoadOptions, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::Path,
    process::Command,
    time::Instant,
};
use thiserror::Error;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEngineProbe {
    pub detected: bool,
    pub provider: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEngineStatus {
    pub pdf: FileEngineProbe,
    pub pdf_compressor: FileEngineProbe,
    pub document: FileEngineProbe,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PdfCompressionPreset {
    High,
    Balanced,
    Small,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfCompressionRequest {
    pub input_path: String,
    pub output_path: String,
    pub preset: PdfCompressionPreset,
    #[serde(default)]
    pub input_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMergeRequest {
    pub input_paths: Vec<String>,
    pub output_path: String,
    #[serde(default)]
    pub input_bytes: Option<Vec<Vec<u8>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSplitRequest {
    pub input_path: String,
    pub output_path: String,
    pub pages: String,
    #[serde(default)]
    pub input_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfToWordRequest {
    pub input_path: String,
    pub output_path: String,
    #[serde(default)]
    pub input_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordToPdfRequest {
    pub input_path: String,
    pub output_path: String,
    #[serde(default)]
    pub input_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesToPdfInput {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesToPdfRequest {
    pub output_path: String,
    pub images: Vec<ImagesToPdfInput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfCompressionResult {
    pub input_path: String,
    pub output_path: String,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Error)]
pub enum FileEngineError {
    #[error("PDF input must be an existing file")]
    InvalidInput,
    #[error("PDF output must use a .pdf extension")]
    InvalidOutput,
    #[error("built-in PDF engine could not read this file")]
    PdfReadFailed,
    #[error("built-in PDF engine could not write the output file")]
    PdfWriteFailed,
    #[error("this PDF contains an image encoding that the built-in engine cannot recompress")]
    ImageUnsupported,
    #[error("PDF output was not created")]
    OutputMissing,
    #[error("at least one input file is required")]
    NoInputFiles,
    #[error("the page range is invalid")]
    InvalidPageRange,
    #[error("an input image could not be decoded")]
    ImageDecodeFailed,
    #[error("this PDF has no extractable text; OCR is required for scanned PDFs")]
    NoExtractableText,
    #[error("Word output must use a .docx extension")]
    InvalidDocxOutput,
    #[error("the built-in Word writer could not create the output file")]
    DocxWriteFailed,
    #[error("the DOCX input could not be read")]
    DocxReadFailed,
    #[error("the DOCX document does not contain readable text")]
    DocxNoText,
    #[error("this DOCX feature is not supported by the built-in converter")]
    DocxUnsupported,
    #[error("file engine filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Probes only fixed executable names. User-provided paths and file contents
/// never cross this boundary, and the command is deliberately not run through a shell.
pub fn detect() -> FileEngineStatus {
    FileEngineStatus {
        pdf: builtin_pdf_probe(),
        pdf_compressor: builtin_pdf_probe(),
        document: probe(&[("libreoffice", "LibreOffice"), ("soffice", "LibreOffice")]),
    }
}

/// Compresses a PDF with the built-in Rust engine. The original input is only
/// read; callers choose a new output path.
pub fn compress_pdf(
    request: PdfCompressionRequest,
) -> Result<PdfCompressionResult, FileEngineError> {
    let input = Path::new(&request.input_path);
    let output = Path::new(&request.output_path);
    if request.input_bytes.is_none() && !input.is_file() {
        return Err(FileEngineError::InvalidInput);
    }
    if output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("pdf")
    {
        return Err(FileEngineError::InvalidOutput);
    }
    if let Some(parent) = output.parent() {
        if !parent.is_dir() {
            return Err(FileEngineError::InvalidOutput);
        }
    }
    if request.input_bytes.is_none() && input.canonicalize().ok() == output.canonicalize().ok() {
        return Err(FileEngineError::InvalidOutput);
    }
    let started = Instant::now();
    let input_bytes = request
        .input_bytes
        .as_deref()
        .map(|bytes| bytes.len() as u64);
    let mut document = match request.input_bytes.as_deref() {
        Some(bytes) => Document::load_mem_with_options(
            bytes,
            LoadOptions::with_max_decompressed_size(256 * 1024 * 1024),
        ),
        None => Document::load_with_options(
            input,
            LoadOptions::with_max_decompressed_size(256 * 1024 * 1024),
        ),
    }
    .map_err(|_| FileEngineError::PdfReadFailed)?;
    let settings = PdfCompressionSettings::for_preset(&request.preset);
    recompress_images(&mut document, settings)?;
    document.compress();
    document
        .save(output)
        .map_err(|_| FileEngineError::PdfWriteFailed)?;
    let output_bytes = fs::metadata(output)
        .map_err(|_| FileEngineError::OutputMissing)?
        .len();
    if output_bytes == 0 {
        return Err(FileEngineError::OutputMissing);
    }
    let input_bytes = input_bytes.unwrap_or(fs::metadata(input)?.len());
    Ok(PdfCompressionResult {
        input_path: request.input_path,
        output_path: request.output_path,
        input_bytes,
        output_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

pub fn merge_pdfs(request: PdfMergeRequest) -> Result<PdfCompressionResult, FileEngineError> {
    if request.input_paths.len() < 2 {
        return Err(FileEngineError::NoInputFiles);
    }
    if let Some(bytes) = &request.input_bytes {
        if bytes.len() != request.input_paths.len() {
            return Err(FileEngineError::InvalidInput);
        }
    }
    let output = validate_pdf_output(&request.output_path)?;
    let mut documents = Vec::with_capacity(request.input_paths.len());
    let mut input_bytes = 0u64;
    for (index, path) in request.input_paths.iter().enumerate() {
        let inline = request
            .input_bytes
            .as_ref()
            .and_then(|items| items.get(index))
            .map(Vec::as_slice);
        input_bytes += inline
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(fs::metadata(path)?.len());
        documents.push(load_document(path, inline)?);
    }
    let started = Instant::now();
    let mut merged = merge_documents(documents)?;
    merged.compress();
    merged
        .save(output)
        .map_err(|_| FileEngineError::PdfWriteFailed)?;
    let output_bytes = output_metadata(output)?;
    Ok(PdfCompressionResult {
        input_path: request.input_paths.join(", "),
        output_path: request.output_path,
        input_bytes,
        output_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

pub fn split_pdf(request: PdfSplitRequest) -> Result<PdfCompressionResult, FileEngineError> {
    let output = validate_pdf_output(&request.output_path)?;
    let input_bytes = request
        .input_bytes
        .as_deref()
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(fs::metadata(&request.input_path)?.len());
    let started = Instant::now();
    let mut document = load_document(&request.input_path, request.input_bytes.as_deref())?;
    let page_count = document.get_pages().len() as u32;
    let selected = parse_page_ranges(&request.pages, page_count)?;
    let pages_to_delete = document
        .get_pages()
        .keys()
        .copied()
        .filter(|page| !selected.contains(page))
        .collect::<Vec<_>>();
    document.delete_pages(&pages_to_delete);
    document.prune_objects();
    document.compress();
    document
        .save(output)
        .map_err(|_| FileEngineError::PdfWriteFailed)?;
    let output_bytes = output_metadata(output)?;
    Ok(PdfCompressionResult {
        input_path: request.input_path,
        output_path: request.output_path,
        input_bytes,
        output_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

/// Extracts text from a text-based PDF into a best-effort DOCX document.
///
/// This intentionally does not claim layout fidelity: PDF text positioning is
/// not a document structure, and scanned pages have no text to extract. The
/// layout pass below uses text coordinates to recover readable lines. Keeping
/// the implementation embedded means this feature does not require Ghostscript,
/// LibreOffice, or an installed command-line runtime.
pub fn pdf_to_word(request: PdfToWordRequest) -> Result<PdfCompressionResult, FileEngineError> {
    let input = Path::new(&request.input_path);
    let output = validate_docx_output(&request.output_path)?;
    if request.input_bytes.is_none() && !input.is_file() {
        return Err(FileEngineError::InvalidInput);
    }
    if request.input_bytes.is_none() && input.canonicalize().ok() == output.canonicalize().ok() {
        return Err(FileEngineError::InvalidDocxOutput);
    }
    let input_bytes = request
        .input_bytes
        .as_deref()
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(
            fs::metadata(input)
                .map_err(|_| FileEngineError::InvalidInput)?
                .len(),
        );
    let started = Instant::now();
    let document = load_document(&request.input_path, request.input_bytes.as_deref())?;
    let pages = document.get_pages();
    let mut page_layouts = Vec::with_capacity(pages.len());
    for (page_number, page_id) in pages {
        let positioned = extract_positioned_page_text(&document, page_id)?;
        let layout = if positioned.is_empty() {
            let text = document
                .extract_text_with_limit(&[page_number], 256 * 1024 * 1024)
                .map_err(|_| FileEngineError::PdfReadFailed)?;
            plain_text_layout(&text)
        } else {
            positioned
        };
        page_layouts.push(layout);
    }
    if page_layouts
        .iter()
        .all(|lines| lines.iter().all(|line| line.spans.is_empty()))
    {
        return Err(FileEngineError::NoExtractableText);
    }
    let docx = build_docx(&page_layouts);
    fs::write(output, docx).map_err(|_| FileEngineError::DocxWriteFailed)?;
    let output_bytes = output_metadata(output)?;
    Ok(PdfCompressionResult {
        input_path: request.input_path,
        output_path: request.output_path,
        input_bytes,
        output_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct WordRun {
    text: String,
    font_size: f32,
    bold: bool,
    italic: bool,
}

#[derive(Debug, Default)]
struct WordParagraph {
    runs: Vec<WordRun>,
    page_break_before: bool,
}

#[derive(Debug)]
struct PdfLine {
    runs: Vec<WordRun>,
    height: f32,
    paragraph_end: bool,
}

/// Converts a DOCX text document to PDF with the embedded PDF writer.
///
/// This is deliberately a text-layout converter, not a promise of full Word
/// rendering compatibility. It supports ordinary paragraphs, line breaks,
/// basic run styles and Unicode text through a standard Chinese CID font.
pub fn word_to_pdf(request: WordToPdfRequest) -> Result<PdfCompressionResult, FileEngineError> {
    let input = Path::new(&request.input_path);
    let output = validate_pdf_output(&request.output_path)?;
    if request.input_bytes.is_none() && !input.is_file() {
        return Err(FileEngineError::DocxReadFailed);
    }
    if request.input_bytes.is_none() && input.canonicalize().ok() == output.canonicalize().ok() {
        return Err(FileEngineError::InvalidOutput);
    }
    let input_bytes = if let Some(bytes) = request.input_bytes.as_deref() {
        bytes.len() as u64
    } else {
        fs::metadata(input)
            .map_err(|_| FileEngineError::DocxReadFailed)?
            .len()
    };
    let started = Instant::now();
    let source = match request.input_bytes.as_deref() {
        Some(bytes) => bytes.to_vec(),
        None => fs::read(input).map_err(|_| FileEngineError::DocxReadFailed)?,
    };
    let xml = read_docx_entry(&source, b"word/document.xml")?;
    let paragraphs = parse_docx_paragraphs(&xml)?;
    if paragraphs
        .iter()
        .all(|paragraph| paragraph.runs.iter().all(|run| run.text.trim().is_empty()))
    {
        return Err(FileEngineError::DocxNoText);
    }
    write_word_pdf(output, &paragraphs)?;
    let output_bytes = output_metadata(output)?;
    Ok(PdfCompressionResult {
        input_path: request.input_path,
        output_path: request.output_path,
        input_bytes,
        output_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

fn read_docx_entry(source: &[u8], wanted_name: &[u8]) -> Result<Vec<u8>, FileEngineError> {
    let eocd = source
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or(FileEngineError::DocxReadFailed)?;
    let entries = zip_u16(source, eocd + 10).ok_or(FileEngineError::DocxReadFailed)? as usize;
    let central_offset =
        zip_u32(source, eocd + 16).ok_or(FileEngineError::DocxReadFailed)? as usize;
    let mut cursor = central_offset;
    for _ in 0..entries {
        if source.get(cursor..cursor + 4) != Some(b"PK\x01\x02") {
            return Err(FileEngineError::DocxReadFailed);
        }
        let flags = zip_u16(source, cursor + 8).ok_or(FileEngineError::DocxReadFailed)?;
        let method = zip_u16(source, cursor + 10).ok_or(FileEngineError::DocxReadFailed)?;
        let compressed_size =
            zip_u32(source, cursor + 20).ok_or(FileEngineError::DocxReadFailed)? as usize;
        let name_size =
            zip_u16(source, cursor + 28).ok_or(FileEngineError::DocxReadFailed)? as usize;
        let extra_size =
            zip_u16(source, cursor + 30).ok_or(FileEngineError::DocxReadFailed)? as usize;
        let comment_size =
            zip_u16(source, cursor + 32).ok_or(FileEngineError::DocxReadFailed)? as usize;
        let local_offset =
            zip_u32(source, cursor + 42).ok_or(FileEngineError::DocxReadFailed)? as usize;
        let name_start = cursor + 46;
        let name_end = name_start
            .checked_add(name_size)
            .ok_or(FileEngineError::DocxReadFailed)?;
        let name = source
            .get(name_start..name_end)
            .ok_or(FileEngineError::DocxReadFailed)?;
        if name == wanted_name {
            if flags & 1 != 0 {
                return Err(FileEngineError::DocxUnsupported);
            }
            if source.get(local_offset..local_offset + 4) != Some(b"PK\x03\x04") {
                return Err(FileEngineError::DocxReadFailed);
            }
            let local_name_size =
                zip_u16(source, local_offset + 26).ok_or(FileEngineError::DocxReadFailed)? as usize;
            let local_extra_size =
                zip_u16(source, local_offset + 28).ok_or(FileEngineError::DocxReadFailed)? as usize;
            let data_start = local_offset
                .checked_add(30)
                .and_then(|value| value.checked_add(local_name_size))
                .and_then(|value| value.checked_add(local_extra_size))
                .ok_or(FileEngineError::DocxReadFailed)?;
            let data_end = data_start
                .checked_add(compressed_size)
                .ok_or(FileEngineError::DocxReadFailed)?;
            let compressed = source
                .get(data_start..data_end)
                .ok_or(FileEngineError::DocxReadFailed)?;
            return match method {
                0 => Ok(compressed.to_vec()),
                8 => {
                    let mut decoder = DeflateDecoder::new(compressed);
                    let mut decoded = Vec::new();
                    decoder
                        .read_to_end(&mut decoded)
                        .map_err(|_| FileEngineError::DocxReadFailed)?;
                    Ok(decoded)
                }
                _ => Err(FileEngineError::DocxUnsupported),
            };
        }
        cursor = name_end
            .checked_add(extra_size)
            .and_then(|value| value.checked_add(comment_size))
            .ok_or(FileEngineError::DocxReadFailed)?;
    }
    Err(FileEngineError::DocxReadFailed)
}

fn zip_u16(source: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *source.get(offset)?,
        *source.get(offset + 1)?,
    ]))
}

fn zip_u32(source: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *source.get(offset)?,
        *source.get(offset + 1)?,
        *source.get(offset + 2)?,
        *source.get(offset + 3)?,
    ]))
}

fn parse_docx_paragraphs(xml: &[u8]) -> Result<Vec<WordParagraph>, FileEngineError> {
    let source = String::from_utf8_lossy(xml);
    let mut paragraphs = Vec::new();
    let mut paragraph: Option<WordParagraph> = None;
    let mut run: Option<WordRun> = None;
    let mut in_text = false;
    let mut cursor = 0usize;
    while cursor < source.len() {
        let Some(relative_start) = source[cursor..].find('<') else {
            if in_text {
                if let Some(current_run) = run.as_mut() {
                    current_run
                        .text
                        .push_str(&decode_xml_entities(&source[cursor..]));
                }
            }
            break;
        };
        let text_end = cursor + relative_start;
        if in_text && text_end > cursor {
            if let Some(current_run) = run.as_mut() {
                current_run
                    .text
                    .push_str(&decode_xml_entities(&source[cursor..text_end]));
            }
        }
        let Some(relative_end) = source[text_end..].find('>') else {
            return Err(FileEngineError::DocxReadFailed);
        };
        let tag_end = text_end + relative_end;
        let tag = &source[text_end + 1..tag_end];
        let (closing, self_closing, name) = docx_tag(tag);
        match (closing, name) {
            (false, "w:p") => {
                if let Some(previous) = paragraph.take() {
                    paragraphs.push(previous);
                }
                paragraph = Some(WordParagraph::default());
            }
            (true, "w:p") => {
                finish_docx_run(&mut paragraph, &mut run);
                if let Some(completed) = paragraph.take() {
                    paragraphs.push(completed);
                }
            }
            (false, "w:r") => {
                finish_docx_run(&mut paragraph, &mut run);
                run = Some(WordRun {
                    text: String::new(),
                    font_size: 11.0,
                    bold: false,
                    italic: false,
                });
            }
            (true, "w:r") => finish_docx_run(&mut paragraph, &mut run),
            (false, "w:b") => {
                if let Some(current_run) = run.as_mut() {
                    current_run.bold = xml_bool_attr(tag);
                }
            }
            (false, "w:i") => {
                if let Some(current_run) = run.as_mut() {
                    current_run.italic = xml_bool_attr(tag);
                }
            }
            (false, "w:sz") => {
                if let (Some(current_run), Some(value)) = (run.as_mut(), xml_attr(tag, "w:val")) {
                    if let Ok(half_points) = value.parse::<f32>() {
                        current_run.font_size = (half_points / 2.0).clamp(5.0, 96.0);
                    }
                }
            }
            (false, "w:br") => {
                if let Some(current_run) = run.as_mut() {
                    if xml_attr(tag, "w:type").as_deref() == Some("page") {
                        if let Some(current_paragraph) = paragraph.as_mut() {
                            current_paragraph.page_break_before = true;
                        }
                    } else {
                        current_run.text.push('\n');
                    }
                }
            }
            (false, "w:tab") => {
                if let Some(current_run) = run.as_mut() {
                    current_run.text.push('\t');
                }
            }
            (_, "w:t") => in_text = !closing,
            _ => {}
        }
        if self_closing && name == "w:br" {
            in_text = false;
        }
        cursor = tag_end + 1;
    }
    finish_docx_run(&mut paragraph, &mut run);
    if let Some(completed) = paragraph {
        paragraphs.push(completed);
    }
    Ok(paragraphs)
}

fn finish_docx_run(paragraph: &mut Option<WordParagraph>, run: &mut Option<WordRun>) {
    if let Some(current_run) = run.take() {
        if let Some(current_paragraph) = paragraph.as_mut() {
            current_paragraph.runs.push(current_run);
        }
    }
}

fn docx_tag(tag: &str) -> (bool, bool, &str) {
    let trimmed = tag.trim();
    let closing = trimmed.starts_with('/');
    let body = trimmed.trim_start_matches('/').trim_end_matches('/').trim();
    let name = body.split_whitespace().next().unwrap_or_default();
    (closing, trimmed.ends_with('/'), name)
}

fn xml_attr(tag: &str, attribute: &str) -> Option<String> {
    let marker = format!("{attribute}=");
    let start = tag.find(&marker)? + marker.len();
    let quote = tag.as_bytes().get(start).copied()? as char;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let value_start = start + 1;
    let value_end = tag[value_start..].find(quote)? + value_start;
    Some(tag[value_start..value_end].to_owned())
}

fn xml_bool_attr(tag: &str) -> bool {
    !matches!(
        xml_attr(tag, "w:val").as_deref(),
        Some("0" | "false" | "off")
    )
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn write_word_pdf(path: &Path, paragraphs: &[WordParagraph]) -> Result<(), FileEngineError> {
    let mut document = Document::with_version("1.5");
    let regular = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let bold = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
    });
    let italic = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Oblique",
    });
    let bold_italic = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-BoldOblique",
    });
    let cjk_descriptor = document.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "STSong-Light", "Flags" => 4,
        "FontBBox" => vec![
            Object::Integer(-25),
            Object::Integer(-254),
            Object::Integer(1000),
            Object::Integer(880),
        ],
        "ItalicAngle" => 0, "Ascent" => 880, "Descent" => -120, "CapHeight" => 700, "StemV" => 80,
    });
    let cjk_descendant = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType0", "BaseFont" => "STSong-Light",
        "CIDSystemInfo" => dictionary! { "Registry" => "Adobe", "Ordering" => "GB1", "Supplement" => 4 },
        "FontDescriptor" => cjk_descriptor, "DW" => 1000,
    });
    let cjk = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "STSong-Light",
        "Encoding" => "UniGB-UCS2-H", "DescendantFonts" => vec![Object::Reference(cjk_descendant)],
    });
    let fonts = dictionary! {
        "F1" => regular, "F2" => bold, "F3" => italic, "F4" => bold_italic, "FC" => cjk,
    };
    let pages_id = document.new_object_id();
    let mut page_ids = Vec::new();
    let mut page_lines: Vec<PdfLine> = Vec::new();
    let mut y = 788.0f32;
    for paragraph in paragraphs {
        if paragraph.page_break_before && !page_lines.is_empty() {
            add_word_pdf_page(&mut document, pages_id, &fonts, &page_lines, &mut page_ids)?;
            page_lines.clear();
            y = 788.0;
        }
        let lines = wrap_word_paragraph(paragraph, 487.0);
        for line in lines {
            if y - line.height < 54.0 && !page_lines.is_empty() {
                add_word_pdf_page(&mut document, pages_id, &fonts, &page_lines, &mut page_ids)?;
                page_lines.clear();
                y = 788.0;
            }
            y -= line.height;
            let paragraph_end = line.paragraph_end;
            page_lines.push(line);
            if paragraph_end {
                y -= 6.0;
            }
        }
    }
    if !page_lines.is_empty() || page_ids.is_empty() {
        add_word_pdf_page(&mut document, pages_id, &fonts, &page_lines, &mut page_ids)?;
    }
    document.set_object(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => page_ids.len() as u32,
        },
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document
        .save(path)
        .map_err(|_| FileEngineError::PdfWriteFailed)?;
    Ok(())
}

fn add_word_pdf_page(
    document: &mut Document,
    pages_id: lopdf::ObjectId,
    fonts: &Dictionary,
    lines: &[PdfLine],
    page_ids: &mut Vec<lopdf::ObjectId>,
) -> Result<(), FileEngineError> {
    let mut content = Content {
        operations: Vec::new(),
    };
    let mut y = 788.0f32;
    for line in lines {
        content.operations.push(Operation::new("BT", vec![]));
        content.operations.push(Operation::new(
            "Tm",
            vec![1.into(), 0.into(), 0.into(), 1.into(), 54.into(), y.into()],
        ));
        for run in &line.runs {
            append_pdf_word_run(&mut content, run);
        }
        content.operations.push(Operation::new("ET", vec![]));
        y -= line.height;
        if line.paragraph_end {
            y -= 6.0;
        }
    }
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        content
            .encode()
            .map_err(|_| FileEngineError::PdfWriteFailed)?,
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => fonts.clone() },
        "Contents" => content_id,
    });
    page_ids.push(page_id);
    Ok(())
}

fn append_pdf_word_run(content: &mut Content<Vec<Operation>>, run: &WordRun) {
    let font_name = if run.bold && run.italic {
        b"F4".as_slice()
    } else if run.bold {
        b"F2".as_slice()
    } else if run.italic {
        b"F3".as_slice()
    } else {
        b"F1".as_slice()
    };
    let mut chunk = String::new();
    let mut unicode = false;
    for character in run.text.chars() {
        let character_unicode = !character.is_ascii();
        if !chunk.is_empty() && character_unicode != unicode {
            append_pdf_word_chunk(content, &chunk, run.font_size, font_name, unicode);
            chunk.clear();
        }
        unicode = character_unicode;
        chunk.push(character);
    }
    if !chunk.is_empty() {
        append_pdf_word_chunk(content, &chunk, run.font_size, font_name, unicode);
    }
}

fn append_pdf_word_chunk(
    content: &mut Content<Vec<Operation>>,
    text: &str,
    font_size: f32,
    latin_font: &[u8],
    unicode: bool,
) {
    content.operations.push(Operation::new(
        "Tf",
        vec![
            Object::Name(if unicode {
                b"FC".to_vec()
            } else {
                latin_font.to_vec()
            }),
            font_size.into(),
        ],
    ));
    content.operations.push(Operation::new(
        "Tj",
        vec![Object::string_literal(encode_pdf_text(text, unicode))],
    ));
}

fn encode_pdf_text(text: &str, unicode: bool) -> Vec<u8> {
    if unicode {
        let mut bytes = vec![0xfe, 0xff];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        bytes
    } else {
        text.chars()
            .map(|character| {
                if character.is_ascii() {
                    character as u8
                } else {
                    b'?'
                }
            })
            .collect()
    }
}

fn wrap_word_paragraph(paragraph: &WordParagraph, max_width: f32) -> Vec<PdfLine> {
    let mut lines = vec![PdfLine {
        runs: Vec::new(),
        height: 15.0,
        paragraph_end: false,
    }];
    let mut width = 0.0f32;
    for run in &paragraph.runs {
        for character in run.text.chars() {
            if character == '\n' {
                lines.push(PdfLine {
                    runs: Vec::new(),
                    height: run.font_size * 1.35,
                    paragraph_end: false,
                });
                width = 0.0;
                continue;
            }
            let character_width = if character == '\t' {
                run.font_size * 2.0
            } else if character.is_ascii() {
                run.font_size * 0.5
            } else {
                run.font_size
            };
            if width + character_width > max_width
                && lines.last().is_some_and(|line| !line.runs.is_empty())
            {
                lines.push(PdfLine {
                    runs: Vec::new(),
                    height: run.font_size * 1.35,
                    paragraph_end: false,
                });
                width = 0.0;
            }
            let text = if character == '\t' {
                "    "
            } else {
                &character.to_string()
            };
            if let Some(last_run) = lines.last_mut().and_then(|line| line.runs.last_mut()) {
                if last_run.font_size == run.font_size
                    && last_run.bold == run.bold
                    && last_run.italic == run.italic
                {
                    last_run.text.push_str(text);
                } else {
                    lines.last_mut().expect("line exists").runs.push(WordRun {
                        text: text.to_owned(),
                        ..run.clone()
                    });
                }
            } else {
                lines.last_mut().expect("line exists").runs.push(WordRun {
                    text: text.to_owned(),
                    ..run.clone()
                });
            }
            lines.last_mut().expect("line exists").height = lines
                .last()
                .map(|line| line.height.max(run.font_size * 1.35))
                .unwrap_or(15.0);
            width += character_width;
        }
    }
    if lines.last().is_some_and(|line| line.runs.is_empty()) && lines.len() > 1 {
        lines.pop();
    }
    if let Some(last) = lines.last_mut() {
        last.paragraph_end = true;
    }
    lines
}

#[derive(Debug)]
struct PositionedText {
    text: String,
    x: f32,
    y: f32,
    font_size: f32,
    bold: bool,
    italic: bool,
    font_family: Option<String>,
}

#[derive(Debug)]
struct PositionedLine {
    y: f32,
    spans: Vec<PositionedText>,
}

struct FontInfo<'a> {
    encoding: lopdf::Encoding<'a>,
    bold: bool,
    italic: bool,
    family: Option<String>,
}

/// Reconstructs a readable line layout from the PDF text operators. PDF has no
/// paragraphs or columns, so coordinates are the best local signal available
/// without relying on a full document-conversion runtime.
fn extract_positioned_page_text(
    document: &Document,
    page_id: lopdf::ObjectId,
) -> Result<Vec<PositionedLine>, FileEngineError> {
    let fonts = document
        .get_page_fonts(page_id)
        .map_err(|_| FileEngineError::PdfReadFailed)?;
    let encodings = fonts
        .into_iter()
        .filter_map(|(name, font)| {
            font.get_font_encoding_with_limit(document, 256 * 1024 * 1024)
                .ok()
                .map(|encoding| {
                    let family = pdf_font_family(font);
                    (
                        name,
                        FontInfo {
                            encoding,
                            bold: font_is_bold(family.as_deref()),
                            italic: font_is_italic(family.as_deref()),
                            family,
                        },
                    )
                })
        })
        .collect::<BTreeMap<_, _>>();
    let content = Content::decode(
        &document
            .get_page_content_with_limit(page_id, 256 * 1024 * 1024)
            .map_err(|_| FileEngineError::PdfReadFailed)?,
    )
    .map_err(|_| FileEngineError::PdfReadFailed)?;
    let mut spans = Vec::new();
    let mut encoding = None;
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    let mut leading = 0.0f32;
    let mut font_size = 12.0f32;

    for operation in content.operations {
        match operation.operator.as_ref() {
            "Tf" => {
                if let Some(name) = operation
                    .operands
                    .first()
                    .and_then(|object| object.as_name().ok())
                {
                    encoding = encodings.get(name);
                }
                if let Some(size) = operation.operands.get(1).and_then(object_number) {
                    font_size = size.abs().max(1.0);
                }
            }
            "Tm" => {
                if operation.operands.len() >= 6 {
                    x = object_number(&operation.operands[4]).unwrap_or(x);
                    y = object_number(&operation.operands[5]).unwrap_or(y);
                }
            }
            "Td" | "TD" => {
                if operation.operands.len() >= 2 {
                    let dx = object_number(&operation.operands[0]).unwrap_or(0.0);
                    let dy = object_number(&operation.operands[1]).unwrap_or(0.0);
                    x += dx;
                    y += dy;
                    if operation.operator == "TD" {
                        leading = -dy;
                    }
                }
            }
            "T*" => y -= leading,
            "Tj" | "TJ" | "'" | "\"" => {
                if operation.operator == "'" || operation.operator == "\"" {
                    y -= leading;
                }
                let Some(current_font) = encoding else {
                    continue;
                };
                let text = decode_text_operands(&current_font.encoding, &operation.operands);
                if text.trim().is_empty() {
                    continue;
                }
                spans.push(PositionedText {
                    text: text.clone(),
                    x,
                    y,
                    font_size,
                    bold: current_font.bold,
                    italic: current_font.italic,
                    font_family: current_font.family.clone(),
                });
                x += text.chars().count() as f32 * font_size * 0.5;
            }
            _ => {}
        }
    }
    if spans.is_empty() {
        return Ok(Vec::new());
    }
    spans.sort_by(|left, right| {
        right
            .y
            .total_cmp(&left.y)
            .then_with(|| left.x.total_cmp(&right.x))
    });
    let mut lines: Vec<PositionedLine> = Vec::new();
    for span in spans {
        let tolerance = span.font_size.max(2.0) * 0.75;
        let same_line = lines
            .last()
            .is_some_and(|line| (line.y - span.y).abs() <= tolerance);
        if same_line {
            let line = lines.last_mut().expect("line exists");
            line.spans.push(span);
            line.spans.sort_by(|left, right| left.x.total_cmp(&right.x));
        } else {
            lines.push(PositionedLine {
                y: span.y,
                spans: vec![span],
            });
        }
    }
    Ok(lines)
}

fn object_number(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(value) => Some(*value as f32),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn decode_text_operands(encoding: &lopdf::Encoding<'_>, operands: &[Object]) -> String {
    let mut text = String::new();
    for operand in operands {
        match operand {
            Object::String(bytes, _) => {
                let _ = encoding.write_to_string(bytes, &mut text);
            }
            Object::Array(items) => {
                text.push_str(&decode_text_operands(encoding, items));
            }
            // TJ numeric operands are kerning/word-spacing adjustments. The
            // layout pass uses their resulting coordinates, so converting them
            // into literal spaces here would duplicate whitespace.
            Object::Integer(_) | Object::Real(_) => {}
            _ => {}
        }
    }
    text
}

fn pdf_font_family(font: &Dictionary) -> Option<String> {
    let name = font.get(b"BaseFont").ok()?.as_name().ok()?;
    let raw = String::from_utf8_lossy(name).into_owned();
    let family = raw
        .split_once('+')
        .map(|(_, value)| value)
        .unwrap_or(raw.as_str())
        .trim_start_matches('/')
        .to_owned();
    (!family.is_empty()).then_some(family)
}

fn font_is_bold(family: Option<&str>) -> bool {
    family.is_some_and(|value| {
        let lower = value.to_ascii_lowercase();
        lower.contains("bold") || lower.contains("black") || lower.contains("demi")
    })
}

fn font_is_italic(family: Option<&str>) -> bool {
    family.is_some_and(|value| {
        let lower = value.to_ascii_lowercase();
        lower.contains("italic") || lower.contains("oblique") || lower.contains("slant")
    })
}

pub fn images_to_pdf(request: ImagesToPdfRequest) -> Result<PdfCompressionResult, FileEngineError> {
    if request.images.is_empty() {
        return Err(FileEngineError::NoInputFiles);
    }
    let output = validate_pdf_output(&request.output_path)?;
    let input_bytes = request
        .images
        .iter()
        .map(|image| image.bytes.len() as u64)
        .sum();
    let started = Instant::now();
    let pages_id = (1, 0);
    let mut document = Document::with_version("1.5");
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Vec::<Object>::new(),
            "Count" => 0,
        }),
    );
    document.max_id = 1;
    let mut page_ids = Vec::with_capacity(request.images.len());
    for (index, image_input) in request.images.iter().enumerate() {
        let image = image::load_from_memory(&image_input.bytes)
            .map_err(|_| FileEngineError::ImageDecodeFailed)?
            .to_rgb8();
        let (width, height) = image.dimensions();
        let mut encoded = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 92)
            .encode_image(&image)
            .map_err(|_| FileEngineError::ImageUnsupported)?;
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width,
                "Height" => height,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            encoded.into_inner(),
        ));
        let name = format!("Im{}", index + 1);
        let content = Content {
            operations: vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![
                        width.into(),
                        0.into(),
                        0.into(),
                        height.into(),
                        0.into(),
                        0.into(),
                    ],
                ),
                Operation::new("Do", vec![Object::Name(name.as_bytes().to_vec())]),
                Operation::new("Q", vec![]),
            ],
        }
        .encode()
        .map_err(|_| FileEngineError::PdfWriteFailed)?;
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
        });
        document
            .add_xobject(page_id, name.as_bytes(), image_id)
            .map_err(|_| FileEngineError::PdfWriteFailed)?;
        page_ids.push(page_id);
    }
    document.set_object(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => page_ids.len() as u32,
        },
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    document.compress();
    document
        .save(output)
        .map_err(|_| FileEngineError::PdfWriteFailed)?;
    let output_bytes = output_metadata(output)?;
    Ok(PdfCompressionResult {
        input_path: request
            .images
            .iter()
            .map(|image| image.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        output_path: request.output_path,
        input_bytes,
        output_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

fn validate_pdf_output(path: &str) -> Result<&Path, FileEngineError> {
    let output = Path::new(path);
    if output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("pdf")
    {
        return Err(FileEngineError::InvalidOutput);
    }
    if output.parent().is_some_and(|parent| !parent.is_dir()) {
        return Err(FileEngineError::InvalidOutput);
    }
    Ok(output)
}

fn validate_docx_output(path: &str) -> Result<&Path, FileEngineError> {
    let output = Path::new(path);
    if output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("docx")
    {
        return Err(FileEngineError::InvalidDocxOutput);
    }
    if output.parent().is_some_and(|parent| !parent.is_dir()) {
        return Err(FileEngineError::InvalidDocxOutput);
    }
    Ok(output)
}

fn plain_text_layout(text: &str) -> Vec<PositionedLine> {
    text.lines()
        .enumerate()
        .map(|(index, line)| PositionedLine {
            y: -(index as f32),
            spans: vec![PositionedText {
                text: line.to_owned(),
                x: 0.0,
                y: -(index as f32),
                font_size: 12.0,
                bold: false,
                italic: false,
                font_family: None,
            }],
        })
        .collect()
}

fn build_docx(page_layouts: &[Vec<PositionedLine>]) -> Vec<u8> {
    let mut document_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    for (page_index, page_layout) in page_layouts.iter().enumerate() {
        if page_index > 0 {
            document_xml.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#);
        }
        if page_layout.is_empty() {
            append_docx_line(
                &mut document_xml,
                &PositionedLine {
                    y: 0.0,
                    spans: Vec::new(),
                },
            );
        } else {
            for line in page_layout {
                append_docx_line(&mut document_xml, line);
            }
        }
    }
    document_xml.push_str(
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#,
    );

    let content_types = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    build_zip_archive(&[
        ("[Content_Types].xml", content_types.to_vec()),
        ("_rels/.rels", root_rels.to_vec()),
        ("word/document.xml", document_xml.into_bytes()),
    ])
}

fn append_docx_line(document_xml: &mut String, line: &PositionedLine) {
    document_xml.push_str("<w:p><w:pPr><w:spacing w:after=\"0\"/></w:pPr>");
    let mut previous_end: Option<f32> = None;
    let mut previous_text: Option<&str> = None;
    for span in &line.spans {
        if let Some(end) = previous_end {
            let gap = span.x - end;
            let previous_has_space = previous_text
                .and_then(|text| text.chars().last())
                .is_some_and(char::is_whitespace);
            let next_has_space = span.text.chars().next().is_some_and(char::is_whitespace);
            if gap > span.font_size * 0.45 && !previous_has_space && !next_has_space {
                let spaces = (gap / (span.font_size * 0.55)).round().clamp(1.0, 24.0) as usize;
                append_docx_run(document_xml, &" ".repeat(spaces), span);
            }
        }
        append_docx_run(document_xml, &span.text, span);
        previous_end = Some(span.x + span.text.chars().count() as f32 * span.font_size * 0.5);
        previous_text = Some(&span.text);
    }
    if line.spans.is_empty() {
        document_xml.push_str("<w:r><w:t></w:t></w:r>");
    }
    document_xml.push_str("</w:p>");
}

fn append_docx_run(document_xml: &mut String, text: &str, span: &PositionedText) {
    document_xml.push_str("<w:r>");
    if span.bold || span.italic || span.font_family.is_some() || span.font_size > 0.0 {
        document_xml.push_str("<w:rPr>");
        if span.bold {
            document_xml.push_str("<w:b/>");
        }
        if span.italic {
            document_xml.push_str("<w:i/>");
        }
        let half_points = (span.font_size * 2.0).round().clamp(8.0, 144.0) as u32;
        document_xml.push_str(&format!(
            "<w:sz w:val=\"{half_points}\"/><w:szCs w:val=\"{half_points}\"/>"
        ));
        if let Some(family) = &span.font_family {
            let family = xml_escape(family);
            document_xml.push_str(&format!(
                "<w:rFonts w:ascii=\"{family}\" w:hAnsi=\"{family}\" w:eastAsia=\"{family}\"/>"
            ));
        }
        document_xml.push_str("</w:rPr>");
    }
    document_xml.push_str("<w:t xml:space=\"preserve\">");
    document_xml.push_str(&xml_escape(text));
    document_xml.push_str("</w:t></w:r>");
}

fn xml_escape(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            *character == '\t' || *character == '\n' || *character == '\r' || *character >= ' '
        })
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn build_zip_archive(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut archive = Vec::new();
    let mut central_directory = Vec::new();
    for (name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32(data);
        let offset = archive.len() as u32;
        push_u32(&mut archive, 0x0403_4b50);
        push_u16(&mut archive, 20);
        push_u16(&mut archive, 0x0800);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u32(&mut archive, crc);
        push_u32(&mut archive, data.len() as u32);
        push_u32(&mut archive, data.len() as u32);
        push_u16(&mut archive, name_bytes.len() as u16);
        push_u16(&mut archive, 0);
        archive.extend_from_slice(name_bytes);
        archive.extend_from_slice(data);

        push_u32(&mut central_directory, 0x0201_4b50);
        push_u16(&mut central_directory, 20);
        push_u16(&mut central_directory, 20);
        push_u16(&mut central_directory, 0x0800);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u32(&mut central_directory, crc);
        push_u32(&mut central_directory, data.len() as u32);
        push_u32(&mut central_directory, data.len() as u32);
        push_u16(&mut central_directory, name_bytes.len() as u16);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u32(&mut central_directory, 0);
        push_u32(&mut central_directory, offset);
        central_directory.extend_from_slice(name_bytes);
    }
    let central_offset = archive.len() as u32;
    archive.extend_from_slice(&central_directory);
    push_u32(&mut archive, 0x0605_4b50);
    push_u16(&mut archive, 0);
    push_u16(&mut archive, 0);
    push_u16(&mut archive, entries.len() as u16);
    push_u16(&mut archive, entries.len() as u16);
    push_u32(&mut archive, central_directory.len() as u32);
    push_u32(&mut archive, central_offset);
    push_u16(&mut archive, 0);
    archive
}

fn push_u16(buffer: &mut Vec<u8>, value: u16) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn output_metadata(path: &Path) -> Result<u64, FileEngineError> {
    let bytes = fs::metadata(path)
        .map_err(|_| FileEngineError::OutputMissing)?
        .len();
    if bytes == 0 {
        return Err(FileEngineError::OutputMissing);
    }
    Ok(bytes)
}

fn load_document(path: &str, inline: Option<&[u8]>) -> Result<Document, FileEngineError> {
    let options = LoadOptions::with_max_decompressed_size(256 * 1024 * 1024);
    match inline {
        Some(bytes) => Document::load_mem_with_options(bytes, options),
        None => Document::load_with_options(path, options),
    }
    .map_err(|_| FileEngineError::PdfReadFailed)
}

fn parse_page_ranges(
    spec: &str,
    page_count: u32,
) -> Result<std::collections::BTreeSet<u32>, FileEngineError> {
    let mut pages = std::collections::BTreeSet::new();
    for token in spec
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let mut range = token.splitn(2, '-');
        let start = range
            .next()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .ok_or(FileEngineError::InvalidPageRange)?;
        let end = range
            .next()
            .map(|value| value.trim().parse::<u32>())
            .transpose()
            .map_err(|_| FileEngineError::InvalidPageRange)?
            .unwrap_or(start);
        if start == 0 || end == 0 || start > end || end > page_count {
            return Err(FileEngineError::InvalidPageRange);
        }
        pages.extend(start..=end);
    }
    if pages.is_empty() {
        return Err(FileEngineError::InvalidPageRange);
    }
    Ok(pages)
}

fn merge_documents(documents: Vec<Document>) -> Result<Document, FileEngineError> {
    let mut merged = Document::with_version("1.5");
    let pages_id = merged.new_object_id();
    let mut page_ids = Vec::new();
    for mut source in documents {
        source.renumber_objects_with(merged.max_id + 1);
        for page_id in source.get_pages().into_values().collect::<Vec<_>>() {
            let Some(Object::Dictionary(mut page)) = source.objects.remove(&page_id) else {
                continue;
            };
            flatten_page_attributes(&mut page, &source);
            page.set("Parent", pages_id);
            merged.objects.insert(page_id, Object::Dictionary(page));
            page_ids.push(page_id);
        }
        for (object_id, object) in source.objects {
            let object_type = object.type_name().unwrap_or_default();
            if object_type == b"Pages"
                || object_type == b"Catalog"
                || object_type == b"Outlines"
                || object_type == b"Outline"
            {
                continue;
            }
            merged.objects.insert(object_id, object);
        }
        merged.max_id = merged
            .objects
            .keys()
            .map(|(number, _)| *number)
            .max()
            .unwrap_or(merged.max_id);
    }
    merged.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => page_ids.len() as u32,
        }),
    );
    let catalog_id = merged.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    merged.trailer.set("Root", catalog_id);
    merged.renumber_objects();
    Ok(merged)
}

fn flatten_page_attributes(page: &mut Dictionary, source: &Document) {
    let inherited = [
        b"Resources".as_slice(),
        b"MediaBox",
        b"CropBox",
        b"Rotate",
        b"UserUnit",
    ];
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    while let Some(parent_id) = parent {
        let Ok(parent_dict) = source.get_dictionary(parent_id) else {
            break;
        };
        for key in inherited {
            if !page.has(key) {
                if let Ok(value) = parent_dict.get(key) {
                    page.set(key, value.clone());
                }
            }
        }
        parent = parent_dict
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok();
    }
}

#[derive(Clone, Copy)]
struct PdfCompressionSettings {
    max_image_dimension: u32,
    jpeg_quality: u8,
}

impl PdfCompressionSettings {
    fn for_preset(preset: &PdfCompressionPreset) -> Self {
        match preset {
            PdfCompressionPreset::High => Self {
                max_image_dimension: 2200,
                jpeg_quality: 88,
            },
            PdfCompressionPreset::Balanced => Self {
                max_image_dimension: 1600,
                jpeg_quality: 76,
            },
            PdfCompressionPreset::Small => Self {
                max_image_dimension: 1200,
                jpeg_quality: 60,
            },
        }
    }
}

fn builtin_pdf_probe() -> FileEngineProbe {
    FileEngineProbe {
        detected: true,
        provider: Some("Built-in Rust PDF engine".to_owned()),
        version: Some(env!("CARGO_PKG_VERSION").to_owned()),
    }
}

fn recompress_images(
    document: &mut Document,
    settings: PdfCompressionSettings,
) -> Result<(), FileEngineError> {
    for object in document.objects.values_mut() {
        let Object::Stream(stream) = object else {
            continue;
        };
        if !is_jpeg_image(stream) || stream.dict.get(b"SMask").is_ok() {
            continue;
        }
        let Ok(image) = image::load_from_memory(&stream.content) else {
            continue;
        };
        let (width, height) = image.dimensions();
        let scale = (settings.max_image_dimension as f32 / width.max(height) as f32).min(1.0);
        let target_width = ((width as f32 * scale).round() as u32).max(1);
        let target_height = ((height as f32 * scale).round() as u32).max(1);
        let resized = if target_width != width || target_height != height {
            image.resize(target_width, target_height, FilterType::Lanczos3)
        } else {
            image
        };
        let mut encoded = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, settings.jpeg_quality)
            .encode_image(&resized)
            .map_err(|_| FileEngineError::ImageUnsupported)?;
        let encoded = encoded.into_inner();
        if encoded.len() >= stream.content.len() {
            continue;
        }
        set_jpeg_image_stream(stream, encoded, target_width, target_height);
    }
    Ok(())
}

fn is_jpeg_image(stream: &Stream) -> bool {
    stream
        .dict
        .get(b"Subtype")
        .is_ok_and(|value| value.as_name().is_ok_and(|name| name == b"Image"))
        && stream
            .filters()
            .is_ok_and(|filters| filters.iter().any(|filter| *filter == b"DCTDecode"))
}

fn set_jpeg_image_stream(stream: &mut Stream, content: Vec<u8>, width: u32, height: u32) {
    stream.set_plain_content(content);
    stream.dict.set("Filter", "DCTDecode");
    stream.dict.set("Width", width);
    stream.dict.set("Height", height);
    stream.dict.set("ColorSpace", "DeviceRGB");
    stream.dict.set("BitsPerComponent", 8);
}

fn probe(candidates: &[(&str, &str)]) -> FileEngineProbe {
    for (executable, provider) in candidates {
        // `Command::new` with a fixed executable and a fixed argument prevents
        // filenames or other external input from becoming command syntax.
        let result = Command::new(executable).arg("--version").output();
        let Ok(output) = result else { continue };
        if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
            continue;
        }
        let version = first_line(&output.stdout).or_else(|| first_line(&output.stderr));
        return FileEngineProbe {
            detected: true,
            provider: Some((*provider).to_owned()),
            version,
        };
    }
    FileEngineProbe {
        detected: false,
        provider: None,
        version: None,
    }
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    let trimmed = line.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        ImagesToPdfInput, ImagesToPdfRequest, PdfCompressionPreset, PdfCompressionRequest,
        PdfMergeRequest, PdfSplitRequest, PdfToWordRequest, WordToPdfRequest, compress_pdf,
        first_line, images_to_pdf, merge_pdfs, parse_page_ranges, pdf_to_word, split_pdf,
        word_to_pdf,
    };
    use image::{ImageBuffer, Rgb};
    use lopdf::{Document, Stream, dictionary};
    use std::{
        collections::BTreeSet,
        fs,
        io::Cursor,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn keeps_only_a_short_sanitized_version_line() {
        let version = first_line(b"Tool 1.2.3\nsecret second line").expect("version");
        assert_eq!(version, "Tool 1.2.3");
    }

    #[test]
    fn ignores_empty_output() {
        assert!(first_line(b"\n\r\n").is_none());
    }

    #[test]
    fn parses_page_ranges_and_rejects_out_of_bounds_pages() {
        assert_eq!(
            parse_page_ranges("1-2, 4", 5).expect("range"),
            BTreeSet::from([1, 2, 4])
        );
        assert!(parse_page_ranges("0,6", 5).is_err());
        assert!(parse_page_ranges("3-1", 5).is_err());
    }

    #[test]
    fn compresses_an_embedded_jpeg_without_an_external_engine() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let input = temp_pdf_path(stamp, "input");
        let output = temp_pdf_path(stamp, "output");
        let image = ImageBuffer::from_pixel(2400, 1600, Rgb([80u8, 120u8, 160u8]));
        let mut jpeg = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode_image(&image)
            .expect("encode fixture");
        let mut document = Document::new();
        document.add_object(Stream::new(
            dictionary! {
                "Subtype" => "Image",
                "Filter" => "DCTDecode",
                "Width" => 2400,
                "Height" => 1600,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            jpeg.into_inner(),
        ));
        document.save(&input).expect("write fixture");

        let result = compress_pdf(PdfCompressionRequest {
            input_path: input.to_string_lossy().into_owned(),
            output_path: output.to_string_lossy().into_owned(),
            preset: PdfCompressionPreset::Balanced,
            input_bytes: None,
        })
        .expect("compress fixture");
        assert!(result.output_bytes < result.input_bytes);
        assert!(output.is_file());
        Document::load(&output).expect("valid compressed PDF");
        let _ = fs::remove_file(input);
        let _ = fs::remove_file(output);
    }

    #[test]
    fn merges_and_splits_pdf_pages() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let first = temp_pdf_path(stamp, "first");
        let second = temp_pdf_path(stamp, "second");
        let merged = temp_pdf_path(stamp, "merged");
        let split = temp_pdf_path(stamp, "split");
        write_page_fixture(&first, 1);
        write_page_fixture(&second, 1);

        merge_pdfs(PdfMergeRequest {
            input_paths: vec![
                first.to_string_lossy().into_owned(),
                second.to_string_lossy().into_owned(),
            ],
            output_path: merged.to_string_lossy().into_owned(),
            input_bytes: None,
        })
        .expect("merge fixture");
        assert_eq!(
            Document::load(&merged)
                .expect("merged PDF")
                .get_pages()
                .len(),
            2
        );

        split_pdf(PdfSplitRequest {
            input_path: merged.to_string_lossy().into_owned(),
            output_path: split.to_string_lossy().into_owned(),
            pages: "2".to_owned(),
            input_bytes: None,
        })
        .expect("split fixture");
        assert_eq!(
            Document::load(&split).expect("split PDF").get_pages().len(),
            1
        );

        for path in [first, second, merged, split] {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn creates_a_pdf_from_an_image() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let output = temp_pdf_path(stamp, "images");
        let image = ImageBuffer::from_pixel(320, 180, Rgb([80u8, 120u8, 160u8]));
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode image");
        images_to_pdf(ImagesToPdfRequest {
            output_path: output.to_string_lossy().into_owned(),
            images: vec![ImagesToPdfInput {
                name: "fixture.png".to_owned(),
                bytes: png.into_inner(),
            }],
        })
        .expect("images to PDF");
        assert_eq!(
            Document::load(&output)
                .expect("image PDF")
                .get_pages()
                .len(),
            1
        );
        let _ = fs::remove_file(output);
    }

    #[test]
    fn exports_text_pdf_to_a_docx_without_an_external_runtime() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let input = temp_pdf_path(stamp, "text");
        let output = temp_docx_path(stamp, "text");
        write_text_page_fixture(&input);

        let result = pdf_to_word(PdfToWordRequest {
            input_path: input.to_string_lossy().into_owned(),
            output_path: output.to_string_lossy().into_owned(),
            input_bytes: None,
        })
        .expect("PDF to Word");
        assert!(result.output_bytes > 0);
        let docx = fs::read(&output).expect("read docx");
        assert!(
            docx.windows(b"Hello local PDF".len())
                .any(|window| window == b"Hello local PDF")
        );
        assert!(
            docx.windows(b"<w:b/>".len())
                .any(|window| window == b"<w:b/>")
        );
        assert!(
            docx.windows(b"<w:sz w:val=\"36\"".len())
                .any(|window| window == b"<w:sz w:val=\"36\"")
        );

        let roundtrip = temp_pdf_path(stamp, "word-roundtrip");
        word_to_pdf(WordToPdfRequest {
            input_path: output.to_string_lossy().into_owned(),
            output_path: roundtrip.to_string_lossy().into_owned(),
            input_bytes: None,
        })
        .expect("Word to PDF");
        Document::load(&roundtrip).expect("valid Word PDF");

        let inline_docx = fs::read(&output).expect("read docx for inline upload");
        let inline_roundtrip = temp_pdf_path(stamp, "word-inline-roundtrip");
        word_to_pdf(WordToPdfRequest {
            input_path: "uploaded-document.docx".to_owned(),
            output_path: inline_roundtrip.to_string_lossy().into_owned(),
            input_bytes: Some(inline_docx),
        })
        .expect("inline Word to PDF");
        Document::load(&inline_roundtrip).expect("valid inline Word PDF");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(output);
        let _ = fs::remove_file(roundtrip);
        let _ = fs::remove_file(inline_roundtrip);
    }

    fn temp_pdf_path(stamp: u128, role: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dev-workbench-file-engine-{stamp}-{role}.pdf"))
    }

    fn temp_docx_path(stamp: u128, role: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dev-workbench-file-engine-{stamp}-{role}.docx"))
    }

    fn write_text_page_fixture(path: &PathBuf) {
        let mut document = Document::new();
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica-Bold",
        });
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 18 Tf 72 720 Td (Hello local PDF) Tj ET".to_vec(),
        ));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Contents" => content_id,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        });
        document.objects.insert(
            pages_id,
            dictionary! {
                "Type" => "Pages",
                "Kids" => vec![lopdf::Object::Reference(page_id)],
                "Count" => 1,
            }
            .into(),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.save(path).expect("write text fixture");
    }

    fn write_page_fixture(path: &PathBuf, count: u32) {
        let mut document = Document::new();
        let pages_id = document.new_object_id();
        let page_ids = (0..count)
            .map(|_| {
                document.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                })
            })
            .collect::<Vec<_>>();
        document.objects.insert(
            pages_id,
            dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.iter().copied().map(lopdf::Object::Reference).collect::<Vec<_>>(),
                "Count" => count,
            }.into(),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.save(path).expect("write page fixture");
    }
}
