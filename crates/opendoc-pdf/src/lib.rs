//! PDF export, built on the layout engine rather than beside it.
//!
//! ADR 0009 feared that a PDF and a screen would drift because two engines
//! decided where the pages broke. ADR 0014 removed the second engine:
//! `opendoc-layout` breaks the pages, in Rust, against a bundled font, and the
//! browser places what it decided. This crate is the consequence — it asks the
//! *same* pass where every line sits and draws it. The PDF agrees with the
//! screen by construction, not by care.
//!
//! Concretely, nothing here measures text, breaks a line, or decides a page.
//! [`opendoc_layout::layout_painted_document`] hands over pages of placed
//! runs; this crate turns those into PDF operators and embeds the faces the
//! measurements came from.
//!
//! ## What it draws
//!
//! Paragraphs, headings and list items with their markers and checkboxes;
//! bold, italic and code runs; alignment, indents (including hanging), line
//! spacing and space before/after; explicit page breaks; tables, as their cell
//! text inside a ruled grid; headers and footers on every page with their
//! page-number fields resolved; and the page geometry the document states.
//!
//! ## What it does not, and says so
//!
//! PNG, JPEG, and bounded static SVG images supplied by the application blob
//! store are decoded and embedded at the rectangle layout assigned them. An
//! unavailable, malformed, overlarge, or unsupported image is drawn as a
//! frame and named in a warning rather than silently disappearing. Block equations are drawn as their
//! LaTeX source, which ADR 0003 makes canonical anyway. Comments and
//! suggestions are not drawn at all. Merged table cells use their one anchor's
//! rectangular grid geometry, so covered cells never duplicate their content
//! on paper. In-flow image wrap is deliberately still drawn as an ordinary
//! block by the shared paginator; each wrapped image is named in a warning
//! rather than making the PDF claim the browser's float geometry.
//! Every one of those is an [`ModelWarning`] on the result, per ADR 0010 —
//! the export says what it could not carry, and the caller shows it.
//!
//! Layout's own honesty carries through: a block whose height
//! `opendoc-layout` estimated rather than measured (a table, an unsized image,
//! an equation, text outside the bundled subset) produces a warning naming the
//! block and the reason, because a page built on an estimate may not match the
//! screen exactly and the user should hear that from the export rather than
//! discover it on paper.
//!
//! ## Not signed
//!
//! ADR 0003 keeps signatures on source state and typed content. A PDF is
//! rendered output and is deliberately not signed.

mod font;
mod spreadsheet;

use std::{collections::BTreeMap, io::Cursor};

use opendoc_core::{Block, BlockKind, Document, ModelWarning};
use opendoc_layout::font::{layout_units_to_twips, FaceId, Fonts, TextStyle};
use opendoc_layout::{EstimateReason, PaintItem, PaintRun, PaintedDocument, RunDecoration};
use pdf_writer::types::{ActionType, AnnotationType, CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use font::{to_glyph_space, Subset};

/// The bytes of one PDF, and everything the format could not carry.
///
/// Warnings ride the result rather than the document, for the reason ADR 0010
/// gives: an export warning describes one command's output, and writing it
/// into `document.warnings` would move the bytes a signature covers.
pub struct PdfExport {
    pub bytes: Vec<u8>,
    pub warnings: Vec<ModelWarning>,
}

/// Bytes and declared media type for one content-addressed image. The PDF
/// writer receives these from the app's blob store; it never reads paths or
/// network locations while exporting a document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdfImage {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub use spreadsheet::export_spreadsheet_pdf;

/// Twips to PDF user space, which is points.
fn pt(twips: i32) -> f32 {
    twips as f32 / 20.0
}

/// Renders a document to a PDF.
///
/// Deterministic: no clock, no random file id, no compression heuristics. The
/// same document produces the same bytes, which is the property the DOCX
/// writer already holds and the only one that makes an export diffable.
pub fn export_pdf(document: &Document) -> PdfExport {
    export_pdf_with_images(document, &BTreeMap::new())
}

/// Render a document and embed supplied image blobs where the shared layout
/// pass placed image blocks. Container formats SVG/BMP/GIF/TIFF/WebP are
/// bounded and rasterized into RGB samples for PDF; the source blob remains
/// owned by the caller and is never rewritten. SVG accepts only static,
/// self-contained vector content: it never loads paths, URLs, fonts, or
/// embedded images. Missing or unsupported assets remain visible frames and
/// are reported in the export warnings.
pub fn export_pdf_with_images(
    document: &Document,
    supplied_images: &BTreeMap<String, PdfImage>,
) -> PdfExport {
    let painted = opendoc_layout::layout_painted_document(document);
    let fonts = Fonts::load();
    let mut warnings = Vec::new();

    // Which glyphs each face actually draws. Collected before anything is
    // written, because a font object has to state its widths up front.
    let mut subsets: BTreeMap<FaceId, Subset> = BTreeMap::new();
    let mut missing: Vec<char> = Vec::new();
    for page in &painted.pages {
        for item in &page.items {
            let PaintItem::Text { runs, .. } = item else {
                continue;
            };
            for run in runs {
                let face = FaceId::of(run.style);
                let subset = subsets.entry(face).or_default();
                for ch in run.text.chars() {
                    let glyph = fonts.glyph_id(ch, face);
                    if glyph == 0 {
                        // `.notdef` is not recorded. It is one glyph for every
                        // uncovered character, so a `ToUnicode` entry for it
                        // would claim that *all* of them are whichever one
                        // reached it first — an Arabic paragraph extracted as
                        // the same letter forty times. Absent text is honest;
                        // wrong text is not, and the warning below names it.
                        if !missing.contains(&ch) {
                            missing.push(ch);
                        }
                        continue;
                    }
                    subset.record(glyph, ch);
                }
            }
        }
    }
    subsets.retain(|_, subset| !subset.is_empty());

    let mut pdf = Pdf::new();
    let mut next = 1i32;
    let mut allocate = || {
        let id = Ref::new(next);
        next += 1;
        id
    };

    let catalog = allocate();
    let tree = allocate();
    let info = allocate();

    // One resource name per face, stable across the document so every page
    // shares one font dictionary.
    let faces: Vec<FaceEntry> = subsets
        .keys()
        .enumerate()
        .map(|(index, face)| FaceEntry {
            face: *face,
            type0: allocate(),
            cid: allocate(),
            descriptor: allocate(),
            file: allocate(),
            to_unicode: allocate(),
            name: format!("F{index}").into_bytes(),
        })
        .collect();

    let images = prepare_images(&painted, supplied_images, &mut warnings, &mut allocate);
    let mut opacities = BTreeMap::new();
    for page in &painted.pages {
        for item in &page.items {
            if let PaintItem::Image {
                opacity_percent, ..
            } = item
            {
                if *opacity_percent < 100 {
                    opacities
                        .entry(*opacity_percent)
                        .or_insert_with(&mut allocate);
                }
            }
        }
    }

    let page_ids: Vec<Ref> = (0..painted.pages.len().max(1))
        .map(|_| allocate())
        .collect();
    let content_ids: Vec<Ref> = page_ids.iter().map(|_| allocate()).collect();

    {
        // One `catalog` call: `pdf-writer` refuses a second object with the
        // same id, which is the right refusal — a PDF with two catalogs is
        // not a PDF.
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if !document.locale.is_empty() {
            root.lang(TextStr(&document.locale));
        }
        root.finish();
    }
    pdf.pages(tree)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);

    let setup = &document.page_setup;
    let media = Rect::new(0.0, 0.0, pt(setup.width.twips()), pt(setup.height.twips()));

    // Drawn before any page object is written, because a page has to state
    // the annotations it carries and the links are only known once the runs
    // have been placed.
    let empty = opendoc_layout::PaintedPage::default();
    let drawn: Vec<(Vec<u8>, Vec<LinkRect>)> = (0..page_ids.len())
        .map(|index| {
            let painted_page = painted.pages.get(index).unwrap_or(&empty);
            draw_page(
                painted_page,
                &fonts,
                &faces,
                &images,
                &opacities,
                setup.height.twips(),
            )
        })
        .collect();
    let link_ids: Vec<Vec<Ref>> = drawn
        .iter()
        .map(|(_, links)| links.iter().map(|_| allocate()).collect())
        .collect();

    for (index, (page_id, content_id)) in page_ids.iter().zip(&content_ids).enumerate() {
        let mut page = pdf.page(*page_id);
        page.parent(tree).media_box(media).contents(*content_id);
        {
            let mut resources = page.resources();
            let mut dict = resources.fonts();
            for entry in &faces {
                dict.pair(Name(&entry.name), entry.type0);
            }
            dict.finish();
            let mut objects = resources.x_objects();
            for image in &images {
                objects.pair(Name(&image.name), image.id);
            }
            objects.finish();
            let mut states = resources.ext_g_states();
            for (opacity, id) in &opacities {
                states.pair(Name(format!("GS{opacity}").as_bytes()), *id);
            }
            states.finish();
            resources.finish();
        }
        if !link_ids[index].is_empty() {
            page.annotations(link_ids[index].iter().copied());
        }
        page.finish();

        pdf.stream(*content_id, &drawn[index].0);
    }

    for (links, ids) in drawn.iter().map(|(_, links)| links).zip(&link_ids) {
        for (link, id) in links.iter().zip(ids) {
            let mut annotation = pdf.annotation(*id);
            annotation.subtype(AnnotationType::Link).rect(link.rect);
            // No visible border: the run is already drawn underlined and in
            // the link colour, exactly as the screen draws it, and a viewer's
            // default black rectangle would be a mark the document never made.
            annotation
                .insert(Name(b"Border"))
                .array()
                .items([0i32, 0, 0]);
            annotation
                .action()
                .action_type(ActionType::Uri)
                .uri(Str(link.uri.as_bytes()));
            annotation.finish();
        }
    }

    for entry in &faces {
        let face = &entry.face;
        let subset = &subsets[face];
        let metrics = fonts.metrics(*face);
        let base = Name(face.postscript_name().as_bytes());

        pdf.type0_font(entry.type0)
            .base_font(base)
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(entry.cid)
            .to_unicode(entry.to_unicode);

        {
            let mut cid_font = pdf.cid_font(entry.cid);
            cid_font
                .subtype(CidFontType::Type2)
                .base_font(base)
                .system_info(SystemInfo {
                    registry: Str(b"Adobe"),
                    ordering: Str(b"Identity"),
                    supplement: 0,
                })
                .font_descriptor(entry.descriptor)
                // The bundled faces carry no vertical metrics and the default
                // is only used for a glyph the `/W` array omits, which cannot
                // happen here: every glyph drawn is listed.
                .default_width(0.0)
                .cid_to_gid_map_predefined(Name(b"Identity"));
            let mut widths = cid_font.widths();
            for (glyph, width) in subset.widths(&fonts, *face) {
                widths.consecutive(glyph, [width]);
            }
            widths.finish();
            cid_font.finish();
        }

        let mut flags = FontFlags::SYMBOLIC;
        if face.is_monospaced() {
            flags |= FontFlags::FIXED_PITCH;
        }
        if face.is_italic() {
            flags |= FontFlags::ITALIC;
        }
        pdf.font_descriptor(entry.descriptor)
            .name(base)
            .flags(flags)
            .bbox(Rect::new(
                to_glyph_space(metrics.x_min),
                to_glyph_space(metrics.y_min),
                to_glyph_space(metrics.x_max),
                to_glyph_space(metrics.y_max),
            ))
            .italic_angle(metrics.italic_angle)
            .ascent(to_glyph_space(metrics.ascent))
            .descent(to_glyph_space(metrics.descent))
            .cap_height(to_glyph_space(metrics.cap_height))
            // `StemV` is required and no bundled face states it; the two
            // values below are the conventional regular/bold stand-ins. It
            // affects nothing but a viewer's synthetic fallback, which cannot
            // happen while the face itself is embedded.
            .stem_v(if face.is_bold() { 165.0 } else { 80.0 })
            .font_file2(entry.file);

        let bytes = face.bytes();
        let mut stream = pdf.stream(entry.file, bytes);
        // `Length1` is the uncompressed TrueType length. The stream is not
        // compressed — `pdf-writer` pulls in no deflate implementation, and a
        // 30 KB face per used weight is not worth a compression dependency
        // that would have to build for wasm32 too.
        stream.pair(Name(b"Length1"), bytes.len() as i32);
        stream.finish();

        let cmap = subset.to_unicode();
        pdf.cmap(entry.to_unicode, &cmap).finish();
    }

    for image in &images {
        let mut object = pdf.image_xobject(image.id, &image.rgb);
        object
            .width(image.width as i32)
            .height(image.height as i32)
            .color_space()
            .device_rgb();
        object.bits_per_component(8);
        if let Some(mask) = image.mask_id {
            object.s_mask(mask);
        }
        object.finish();
        if let Some((mask_id, alpha)) = image.mask_id.zip(image.alpha.as_deref()) {
            let mut mask = pdf.image_xobject(mask_id, alpha);
            mask.width(image.width as i32)
                .height(image.height as i32)
                .color_space()
                .device_gray();
            mask.bits_per_component(8);
            mask.finish();
        }
    }

    for (opacity, id) in opacities {
        let alpha = f32::from(opacity) / 100.0;
        pdf.ext_graphics(id)
            .stroking_alpha(alpha)
            .non_stroking_alpha(alpha);
    }

    pdf.document_info(info)
        .title(TextStr(&document.title))
        .producer(TextStr("OpenDoc"));

    collect_warnings(document, &painted, &missing, &mut warnings);

    PdfExport {
        bytes: pdf.finish(),
        warnings,
    }
}

/// One embedded face: which one it is, the five objects it needs, and the
/// resource name pages refer to it by.
struct FaceEntry {
    face: FaceId,
    type0: Ref,
    cid: Ref,
    descriptor: Ref,
    file: Ref,
    to_unicode: Ref,
    name: Vec<u8>,
}

struct PdfImageEntry {
    hash: String,
    id: Ref,
    mask_id: Option<Ref>,
    name: Vec<u8>,
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

struct RasterImage {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

/// Decode only the formats PDF export presently promises. Their dimensions are
/// read and bounded *before* decoding pixel samples, which turns hostile
/// dimensions into a normal export warning rather than a pixel-buffer
/// allocation attempt. Rasterizing BMP, GIF, TIFF and WebP changes their
/// container representation (and selects GIF's first frame), but not the
/// caller-owned source bytes.
fn prepare_images(
    painted: &PaintedDocument,
    supplied: &BTreeMap<String, PdfImage>,
    warnings: &mut Vec<ModelWarning>,
    allocate: &mut impl FnMut() -> Ref,
) -> Vec<PdfImageEntry> {
    const MAX_IMAGE_PIXELS: u64 = 100_000_000;
    let mut hashes = Vec::new();
    for page in &painted.pages {
        for item in &page.items {
            if let PaintItem::Image { blob_hash, .. } = item {
                if !hashes.contains(blob_hash) {
                    hashes.push(blob_hash.clone());
                }
            }
        }
    }
    let mut entries = Vec::new();
    for hash in hashes {
        let Some(source) = supplied.get(&hash) else {
            warnings.push(ModelWarning {
                code: "pdf-image-not-drawn".to_string(),
                message: format!(
                    "image blob {hash} is unavailable, so PDF drew a placeholder frame"
                ),
            });
            continue;
        };
        let media_type = image_media_type_essence(&source.media_type);
        if media_type == "image/svg+xml" {
            let raster = match rasterize_svg(&source.bytes, MAX_IMAGE_PIXELS) {
                Ok(image) => image,
                Err(reason) => {
                    warnings.push(ModelWarning {
                        code: "pdf-svg-not-rasterized".to_string(),
                        message: format!(
                            "SVG image blob {hash} was left as a placeholder frame for PDF export: {reason}"
                        ),
                    });
                    continue;
                }
            };
            warnings.push(ModelWarning {
                code: "pdf-image-rasterized".to_string(),
                message: format!(
                    "image blob {hash} was rasterized from image/svg+xml into PDF RGB samples; the stored source bytes were not changed"
                ),
            });
            entries.push(PdfImageEntry {
                hash,
                id: allocate(),
                mask_id: raster.alpha.as_ref().map(|_| allocate()),
                name: format!("Im{}", entries.len()).into_bytes(),
                width: raster.width,
                height: raster.height,
                rgb: raster.rgb,
                alpha: raster.alpha,
            });
            continue;
        }
        let format = match media_type.as_str() {
            "image/png" => image::ImageFormat::Png,
            "image/jpeg" | "image/jpg" => image::ImageFormat::Jpeg,
            "image/bmp" => image::ImageFormat::Bmp,
            "image/gif" => image::ImageFormat::Gif,
            "image/tiff" => image::ImageFormat::Tiff,
            "image/webp" => image::ImageFormat::WebP,
            other => {
                warnings.push(ModelWarning {
                    code: "pdf-image-unsupported-format".to_string(),
                    message: format!("image blob {hash} has media type {other}, which PDF export cannot embed yet"),
                });
                continue;
            }
        };
        let reader = image::ImageReader::with_format(Cursor::new(&source.bytes), format);
        let (width, height) = match reader.into_dimensions() {
            Ok(dimensions) => dimensions,
            Err(error) => {
                warnings.push(ModelWarning {
                    code: "pdf-image-decode-failed".to_string(),
                    message: format!("image blob {hash} could not be read for PDF export: {error}"),
                });
                continue;
            }
        };
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
            warnings.push(ModelWarning {
                code: "pdf-image-too-large".to_string(),
                message: format!("image blob {hash} has unsupported dimensions {width}×{height}"),
            });
            continue;
        }
        let decoded = match image::load_from_memory_with_format(&source.bytes, format) {
            Ok(image) => image,
            Err(error) => {
                warnings.push(ModelWarning {
                    code: "pdf-image-decode-failed".to_string(),
                    message: format!(
                        "image blob {hash} could not be decoded for PDF export: {error}"
                    ),
                });
                continue;
            }
        };
        let pixels = decoded.to_rgba8();
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        let mut alpha = Vec::with_capacity((width as usize) * (height as usize));
        let mut opaque = true;
        let (rgba_pixels, _) = pixels.as_raw().as_chunks::<4>();
        for rgba in rgba_pixels {
            rgb.extend_from_slice(&rgba[..3]);
            alpha.push(rgba[3]);
            opaque &= rgba[3] == u8::MAX;
        }
        let alpha = (!opaque).then_some(alpha);
        if matches!(
            media_type.as_str(),
            "image/bmp" | "image/gif" | "image/tiff" | "image/webp"
        ) {
            warnings.push(ModelWarning {
                code: "pdf-image-rasterized".to_string(),
                message: format!(
                    "image blob {hash} was decoded from {} into PDF RGB samples; the stored source bytes were not changed",
                    source.media_type
                ),
            });
        }
        // The still-image decoder chooses one raster from containers that can
        // carry more than one. PDF receives that one XObject; retaining the
        // original caller-owned blob does not give PDF GIF animation frames
        // or TIFF pages.
        if matches!(media_type.as_str(), "image/gif" | "image/tiff") {
            warnings.push(ModelWarning {
                code: "pdf-image-first-frame-only".to_string(),
                message: format!(
                    "image blob {hash} was exported from only the first {} in its {}; later animation frames or image pages are not represented in PDF",
                    if media_type == "image/gif" { "frame" } else { "page" },
                    media_type,
                ),
            });
        }
        entries.push(PdfImageEntry {
            hash,
            id: allocate(),
            mask_id: alpha.as_ref().map(|_| allocate()),
            name: format!("Im{}", entries.len()).into_bytes(),
            width,
            height,
            rgb,
            alpha,
        });
    }
    entries
}

/// MIME tokens are case-insensitive and parameters describe how a producer
/// labelled bytes, not a different image container. The original declaration
/// stays in the blob source; PDF only needs this normalized dispatch key to
/// select a decoder.
fn image_media_type_essence(media_type: &str) -> String {
    media_type
        .split_once(';')
        .map_or(media_type, |(essence, _)| essence)
        .trim()
        .to_ascii_lowercase()
}

/// Rasterize a static, self-contained SVG without granting it any ambient
/// authority. The source-size and output-pixel limits are checked before a
/// pixmap is allocated. Embedded images are refused as well: accepting their
/// data URLs would add a second, independently bounded decoder path, and the
/// renderer's normal string resolver is deliberately replaced so it cannot
/// read a local file. SVGZ is also refused rather than attempting to bound a
/// decompression ratio.
fn rasterize_svg(source: &[u8], max_pixels: u64) -> Result<RasterImage, String> {
    const MAX_SVG_BYTES: usize = 10 * 1024 * 1024;
    if source.len() > MAX_SVG_BYTES {
        return Err(format!("source exceeds the {MAX_SVG_BYTES}-byte limit"));
    }
    if source.starts_with(&[0x1f, 0x8b]) {
        return Err("compressed SVGZ is not accepted".to_string());
    }
    let text = std::str::from_utf8(source).map_err(|_| "source is not UTF-8 SVG".to_string())?;
    let lower = text.to_ascii_lowercase();
    if lower.contains("<!doctype") {
        return Err("DOCTYPE declarations are not accepted".to_string());
    }
    if lower.contains("<image") {
        return Err("embedded or external SVG images are not accepted".to_string());
    }

    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(source, &options)
        .map_err(|error| format!("could not parse static SVG: {error}"))?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > max_pixels {
        return Err(format!("unsupported dimensions {width}×{height}"));
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("could not allocate a {width}×{height} raster canvas"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap.take_demultiplied();
    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
    let mut alpha = Vec::with_capacity((width as usize) * (height as usize));
    let mut opaque = true;
    let (rgba_pixels, []) = rgba.as_chunks::<4>() else {
        return Err("renderer produced an incomplete RGBA pixel".to_string());
    };
    for rgba in rgba_pixels {
        rgb.extend_from_slice(&rgba[..3]);
        alpha.push(rgba[3]);
        opaque &= rgba[3] == u8::MAX;
    }
    Ok(RasterImage {
        width,
        height,
        rgb,
        alpha: (!opaque).then_some(alpha),
    })
}

/// A `/Link` annotation the page has to carry: where it is and where it goes.
struct LinkRect {
    rect: Rect,
    uri: String,
}

/// Turns one painted page into a content stream, plus the link annotations
/// the page needs.
///
/// The only arithmetic is the flip from the layout's y-down twips to PDF's
/// y-up points, and the rectangles a highlight, an underline and a link need
/// — all of them derived from the run's own `width_twips`, which the layout
/// measured, so nothing here re-measures anything.
fn draw_page(
    page: &opendoc_layout::PaintedPage,
    fonts: &Fonts,
    faces: &[FaceEntry],
    images: &[PdfImageEntry],
    opacities: &BTreeMap<u8, Ref>,
    page_height_twips: i32,
) -> (Vec<u8>, Vec<LinkRect>) {
    let mut content = Content::new();
    let mut links = Vec::new();
    let flip = |y_twips: i32| pt(page_height_twips - y_twips);
    // The fill colour is document state in a content stream, so it is tracked
    // rather than set per run: a `rg` before every `Tj` would triple the
    // stream for no change in what is drawn.
    let mut fill: Option<[f32; 3]> = None;
    let mut set_fill = |content: &mut Content, colour: [f32; 3]| {
        if fill != Some(colour) {
            content.set_fill_rgb(colour[0], colour[1], colour[2]);
            fill = Some(colour);
        }
    };
    const BLACK: [f32; 3] = [0.0, 0.0, 0.0];
    for item in &page.items {
        match item {
            PaintItem::Image {
                blob_hash,
                alt_text,
                x_twips,
                y_twips,
                width_twips,
                height_twips,
                rotation_degrees,
                crop,
                opacity_percent,
                ..
            } => {
                // This is deliberately a bounded accessibility projection,
                // not a claim that the PDF has a complete tagged-PDF
                // structure tree. `/ActualText` still lets a reader replace
                // a drawn figure (including a warned placeholder) with the
                // model's accessible image text in copy/accessibility flows.
                let has_alt_text = !alt_text.is_empty();
                if has_alt_text {
                    content
                        .begin_marked_content_with_properties(Name(b"Figure"))
                        .properties()
                        .actual_text(TextStr(alt_text));
                } else {
                    // An empty model alt is intentional decoration, not a
                    // missing description.  Keep it out of an assistive
                    // reader's content flow instead of leaving an untagged
                    // painted object for a future tagged-PDF projection to
                    // guess about.  This remains deliberately narrower than
                    // a document structure tree (see the Figure branch).
                    content.begin_marked_content(Name(b"Artifact"));
                }
                if let Some(image) = images.iter().find(|image| image.hash == *blob_hash) {
                    content.save_state();
                    if *opacity_percent < 100 && opacities.contains_key(opacity_percent) {
                        content.set_parameters(Name(format!("GS{opacity_percent}").as_bytes()));
                    }
                    let width = pt(*width_twips);
                    let height = pt(*height_twips);
                    let lower_y = flip(*y_twips + *height_twips);
                    let transform = if *rotation_degrees == 0 {
                        [width, 0.0, 0.0, height, pt(*x_twips), lower_y]
                    } else {
                        let radians = f32::from(*rotation_degrees).to_radians();
                        let (sin, cos) = radians.sin_cos();
                        let a = cos * width;
                        let b = sin * width;
                        let c = -sin * height;
                        let d = cos * height;
                        let center_x = pt(*x_twips) + width / 2.0;
                        let center_y = lower_y + height / 2.0;
                        [
                            a,
                            b,
                            c,
                            d,
                            center_x - (a + c) / 2.0,
                            center_y - (b + d) / 2.0,
                        ]
                    };
                    content.transform(transform);
                    if let Some(crop) = crop {
                        let left = f32::from(crop.left_percent) / 100.0;
                        let right = f32::from(crop.right_percent) / 100.0;
                        let top = f32::from(crop.top_percent) / 100.0;
                        let bottom = f32::from(crop.bottom_percent) / 100.0;
                        // The XObject is drawn in a 0..1 square after its
                        // placement transform. Clipping here, rather than in
                        // page coordinates before that transform, makes a
                        // crop rotate with its image just as CSS clip-path
                        // does.
                        content.rect(left, bottom, 1.0 - left - right, 1.0 - top - bottom);
                        content.clip_nonzero();
                        content.end_path();
                    }
                    content.x_object(Name(&image.name));
                    content.restore_state();
                } else {
                    content.set_line_width(0.5);
                    content.rect(
                        pt(*x_twips),
                        flip(*y_twips + *height_twips),
                        pt(*width_twips),
                        pt(*height_twips),
                    );
                    content.stroke();
                }
                content.end_marked_content();
            }
            PaintItem::Text {
                baseline_twips,
                runs,
            } => {
                // Backgrounds first, under the whole line: a highlight drawn
                // after its own text would paint over it.
                for run in runs {
                    let Some(background) = run.decoration.background else {
                        continue;
                    };
                    let (above, below) = fonts.content_box_twips(run.style);
                    let baseline = *baseline_twips - run_rise(&run.decoration);
                    set_fill(&mut content, background.components());
                    content.rect(
                        pt(run.x_twips),
                        flip(baseline + below),
                        pt(run.width_twips),
                        pt(above + below),
                    );
                    content.fill_nonzero();
                }
                content.begin_text();
                let mut current: Option<(FaceId, i32)> = None;
                for run in runs {
                    let face = FaceId::of(run.style);
                    if current != Some((face, run.style.size_twips)) {
                        let Some(name) = resource_name(faces, face) else {
                            continue;
                        };
                        content.set_font(Name(name), pt(run.style.size_twips));
                        current = Some((face, run.style.size_twips));
                    }
                    set_fill(
                        &mut content,
                        run.decoration
                            .color
                            .map(|colour| colour.components())
                            .unwrap_or(BLACK),
                    );
                    // An absolute text matrix rather than a relative `Td`, so
                    // a run's position never depends on the run before it. The
                    // rise is the `vertical-align` shift the layout measured
                    // the line box with, so a superscript sits where the
                    // screen puts it rather than on the baseline.
                    content.set_text_matrix([
                        1.0,
                        0.0,
                        0.0,
                        1.0,
                        pt(run.x_twips),
                        flip(*baseline_twips - run_rise(&run.decoration)),
                    ]);
                    content.show(Str(&glyph_string(&run.text, run.style, fonts)));
                }
                content.end_text();
                // Rules and link rectangles last, over the text they belong to.
                for run in runs {
                    let baseline = *baseline_twips - run_rise(&run.decoration);
                    if run.decoration.underline || run.decoration.strike {
                        let metrics = fonts.metrics(FaceId::of(run.style));
                        set_fill(
                            &mut content,
                            run.decoration
                                .color
                                .map(|colour| colour.components())
                                .unwrap_or(BLACK),
                        );
                        if run.decoration.underline {
                            rule(
                                &mut content,
                                &flip,
                                run,
                                baseline
                                    - fonts.design_units_to_twips(
                                        metrics.underline_position,
                                        run.style,
                                    ),
                                fonts
                                    .design_units_to_twips(metrics.underline_thickness, run.style)
                                    .max(1),
                            );
                        }
                        if run.decoration.strike {
                            rule(
                                &mut content,
                                &flip,
                                run,
                                baseline
                                    - fonts.design_units_to_twips(
                                        metrics.strikeout_position,
                                        run.style,
                                    ),
                                fonts
                                    .design_units_to_twips(metrics.strikeout_thickness, run.style)
                                    .max(1),
                            );
                        }
                    }
                    if let Some(uri) = run.decoration.link.as_deref() {
                        let (above, below) = fonts.content_box_twips(run.style);
                        links.push(LinkRect {
                            rect: Rect::new(
                                pt(run.x_twips),
                                flip(baseline + below),
                                pt(run.x_twips + run.width_twips),
                                flip(baseline - above),
                            ),
                            uri: uri.to_string(),
                        });
                    }
                }
            }
            PaintItem::Fill {
                x_twips,
                y_twips,
                width_twips,
                height_twips,
                color,
                dashed,
            } => {
                set_fill(
                    &mut content,
                    color.map(opendoc_layout::Rgb::components).unwrap_or(BLACK),
                );
                if *dashed {
                    // A dashed rule is a stroke, not a fill: the explicit
                    // page break draws `border-top: … dashed` on screen, and
                    // a solid bar would be a different mark. Four points on,
                    // four off, which is what Chrome draws a 1px dashed
                    // border as at this scale.
                    content.save_state();
                    content.set_line_width(pt(*height_twips));
                    content.set_dash_pattern([3.0, 3.0], 0.0);
                    let middle = flip(y_twips + height_twips / 2);
                    content.move_to(pt(*x_twips), middle);
                    content.line_to(pt(x_twips + width_twips), middle);
                    content.stroke();
                    content.restore_state();
                } else {
                    content.rect(
                        pt(*x_twips),
                        flip(y_twips + height_twips),
                        pt(*width_twips),
                        pt(*height_twips),
                    );
                    content.fill_nonzero();
                }
            }
            PaintItem::Stroke {
                x_twips,
                y_twips,
                width_twips,
                height_twips,
                line_twips,
            } => {
                content.set_line_width(pt(*line_twips));
                content.rect(
                    pt(*x_twips),
                    flip(y_twips + height_twips),
                    pt(*width_twips),
                    pt(*height_twips),
                );
                content.stroke();
            }
            PaintItem::Edge {
                x1_twips,
                y1_twips,
                x2_twips,
                y2_twips,
                thickness_twips,
                color,
                dash,
            } => {
                // Saved and restored around the whole edge: the stroke colour
                // and the dash pattern are graphics state, and a cell border
                // that left either behind would repaint the next checkbox
                // grey and dashed. Nothing is decided here — the thickness,
                // the colour and the dash lengths are all the layout's, which
                // is what keeps a 2.25pt red dashed border on paper the same
                // border the screen drew.
                content.save_state();
                let [red, green, blue] = color.components();
                content.set_stroke_rgb(red, green, blue);
                content.set_line_width(pt(*thickness_twips));
                if let Some([on, off]) = dash {
                    content.set_dash_pattern([pt(*on), pt(*off)], 0.0);
                }
                content.move_to(pt(*x1_twips), flip(*y1_twips));
                content.line_to(pt(*x2_twips), flip(*y2_twips));
                content.stroke();
                content.restore_state();
            }
        }
    }
    (content.finish().to_vec(), links)
}

/// One underline or strikethrough, as a filled bar the width of the run.
fn rule(
    content: &mut Content,
    flip: &impl Fn(i32) -> f32,
    run: &PaintRun,
    top_twips: i32,
    thickness_twips: i32,
) {
    content.rect(
        pt(run.x_twips),
        flip(top_twips + thickness_twips),
        pt(run.width_twips),
        pt(thickness_twips),
    );
    content.fill_nonzero();
}

/// A run's baseline shift in twips, positive upwards.
fn run_rise(decoration: &RunDecoration) -> i32 {
    layout_units_to_twips(decoration.rise_units)
}

fn resource_name(faces: &[FaceEntry], face: FaceId) -> Option<&[u8]> {
    faces
        .iter()
        .find(|entry| entry.face == face)
        .map(|entry| entry.name.as_slice())
}

/// A run's text as Identity-H glyph ids: two big-endian bytes each.
fn glyph_string(text: &str, style: TextStyle, fonts: &Fonts) -> Vec<u8> {
    let face = FaceId::of(style);
    let mut out = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        out.extend_from_slice(&fonts.glyph_id(ch, face).to_be_bytes());
    }
    out
}

/// Everything the PDF could not carry exactly.
///
/// Deliberately specific: "a table's row heights are estimated" with the
/// block's id beats "this document may not match the screen", because only the
/// first tells the user where to look.
fn collect_warnings(
    document: &Document,
    painted: &PaintedDocument,
    missing: &[char],
    warnings: &mut Vec<ModelWarning>,
) {
    warnings.extend(painted.warnings.iter().cloned());
    for estimate in &painted.estimates {
        warnings.push(ModelWarning {
            code: format!("pdf-estimated-{}", estimate.reason.as_str()),
            message: format!(
                "block {} was placed from an estimate, not a measurement: {}",
                estimate.block_id,
                estimate.reason.description()
            ),
        });
    }
    if !painted.exact
        && painted
            .estimates
            .iter()
            .all(|estimate| estimate.reason != EstimateReason::Suggestions)
        && painted.estimates.is_empty()
    {
        warnings.push(ModelWarning {
            code: "pdf-estimated-layout".to_string(),
            message: "some of this document's geometry was estimated rather than measured"
                .to_string(),
        });
    }
    count_blocks(&document.blocks, warnings);
    let footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .count();
    if footnotes > 0 {
        warnings.push(ModelWarning {
            code: "pdf-footnotes-after-the-body".to_string(),
            message: format!(
                "{footnotes} footnote body/bodies are drawn after the last block rather than at the foot of the page that references them: the editing surface keeps them in one area below the page stack, and this export places them the same way"
            ),
        });
    }
    if !missing.is_empty() {
        let sample: String = missing.iter().take(20).collect();
        warnings.push(ModelWarning {
            code: "pdf-glyph-outside-bundled-font".to_string(),
            message: format!(
                "{} character(s) are outside the bundled font subset: they print as a blank glyph and cannot be copied out of the file, because no `ToUnicode` entry may claim what `.notdef` stands for: {sample}",
                missing.len()
            ),
        });
    }
    if !document.comments.is_empty() {
        warnings.push(ModelWarning {
            code: "pdf-comments-not-drawn".to_string(),
            message: format!(
                "{} comment thread(s) are not drawn: a PDF page has no margin to hang them in",
                document.comments.len()
            ),
        });
    }
    if !document.suggestions.is_empty() {
        warnings.push(ModelWarning {
            code: "pdf-suggestions-not-drawn".to_string(),
            message: format!(
                "{} suggestion(s) are not drawn; the page shows the text as it stands",
                document.suggestions.len()
            ),
        });
    }
}

/// The border width `opendoc-layout` builds every cell's box out of, which is
/// the one the stylesheet's `td` rule states.
fn default_cell_border_twips() -> i32 {
    opendoc_layout::style::TypeScale::default().cell_border
}

/// How many cell border properties state a *used* width other than the one the
/// layout reserved room for.
///
/// Used width, not stated width: CSS computes a `none` border's width to zero,
/// and a turned-off edge on the table's rim really does make the table
/// narrower or shorter on screen than the box this crate drew from.
fn unreserved_border_widths(
    rows: &[opendoc_core::TableRow],
    table_properties: &opendoc_core::TableProperties,
) -> usize {
    let default = default_cell_border_twips();
    // `opendoc-layout` gives an unstated cell edge the table's border before
    // it falls back to the stylesheet grid.  Looking only at the cell's own
    // four optional edges missed that inherited case: a thick table border
    // was painted correctly but the layout had still reserved the default
    // 0.75pt for every edge, with no export warning to name the mismatch.
    let inherited = table_properties.border;
    rows.iter()
        .flat_map(|row| row.cells.iter())
        .flat_map(|cell| {
            let properties = &cell.properties;
            [
                properties.border_top,
                properties.border_bottom,
                properties.border_start,
                properties.border_end,
            ]
        })
        .filter(|border| {
            let used = match border.or(inherited) {
                // Neither model layer states an edge, so CSS supplies the
                // same default width the layout reserved.
                None => default,
                Some(border) if border.style() == opendoc_core::BorderStyle::None => 0,
                Some(border) => border.width().twips(),
            };
            used != default
        })
        .count()
}

fn count_blocks(blocks: &[Block], warnings: &mut Vec<ModelWarning>) {
    for block in blocks {
        match &block.kind {
            BlockKind::Image { layout, .. } => {
                if matches!(
                    layout.effective_placement(),
                    opendoc_core::ImagePlacement::WrapStart
                        | opendoc_core::ImagePlacement::WrapEnd
                ) {
                    warnings.push(ModelWarning {
                        code: "pdf-image-wrap-not-laid-out".to_string(),
                        message: format!(
                            "image {} uses {} text wrapping{}; PDF currently draws it as an ordinary block because the shared paginator does not yet shape following text around in-flow images",
                            block.id,
                            layout.effective_placement().as_str(),
                            if layout.wrap_clearance.is_some() { " with authored clearance" } else { "" },
                        ),
                    });
                }
            }
            BlockKind::EquationBlock { .. } => warnings.push(ModelWarning {
                code: "pdf-equation-drawn-as-source".to_string(),
                message: format!(
                    "equation block {} is drawn as its LaTeX source: there is no math typesetter here",
                    block.id
                ),
            }),
            BlockKind::Table {
                rows, properties, ..
            } => {
                let unreserved = unreserved_border_widths(rows, properties);
                if unreserved > 0 {
                    warnings.push(ModelWarning {
                        code: "pdf-cell-border-width-not-measured".to_string(),
                        message: format!(
                            "table {} has {unreserved} effective cell border(s) whose used width is not the {} the layout reserves on every cell edge — a table or cell rule that is thicker or thinner, or one turned off, whose used width is zero: each is drawn as it states, but the row heights and column positions were measured with the default, so this table's own size can differ from the screen's",
                            block.id,
                            opendoc_layout::style::css_pt(default_cell_border_twips()),
                        ),
                    });
                }
                for row in rows {
                    for cell in &row.cells {
                        count_blocks(&cell.blocks, warnings);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
