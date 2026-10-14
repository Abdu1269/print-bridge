use crate::protocol::EffectivePaper;
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageFormat, ImageReader, Rgb};
use printpdf::{
    ops::PdfFontHandle, BuiltinFont, Color, Line, LinePoint, Mm, Op, PdfDocument, PdfPage,
    PdfSaveOptions, Point, Pt, RawImage, RawImageData, RawImageFormat, Rgb as PdfRgb, TextItem,
    XObjectTransform,
};
use std::{fs, io::Read, path::Path};
use thiserror::Error;

const HEADER_SIZE: usize = 8;
const DEFAULT_DPI: f32 = 203.0;

/// Document formats that Agent can validate or normalize prior to printing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    Pdf,
    Png,
    Jpeg,
}

/// Rectangular placement area used to fit an image within target paper dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Errors returned when detecting or normalizing printable documents.
#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("unsupported document format")]
    UnsupportedFormat,
    #[error("invalid image dimensions")]
    InvalidImageDimensions,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

/// Unified result type for document detection and PDF generation.
pub type DocumentResult<T> = Result<T, DocumentError>;

/// Detect document format based on leading magic bytes.
pub fn detect_format_from_bytes(bytes: &[u8]) -> Option<DocumentFormat> {
    if bytes.starts_with(b"%PDF-") {
        Some(DocumentFormat::Pdf)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(DocumentFormat::Png)
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(DocumentFormat::Jpeg)
    } else {
        None
    }
}

/// Detect file format by reading only the signature header.
pub fn detect_format(path: &Path) -> DocumentResult<Option<DocumentFormat>> {
    let mut header = [0_u8; HEADER_SIZE];
    let bytes_read = fs::File::open(path)?.read(&mut header)?;

    Ok(detect_format_from_bytes(&header[..bytes_read]))
}

/// Calculate centered layout rectangle that fully fits image within the page.
pub fn fit_contain(
    image_width: f64,
    image_height: f64,
    page_width: f64,
    page_height: f64,
) -> DocumentResult<FitRect> {
    if !is_positive_finite(image_width)
        || !is_positive_finite(image_height)
        || !is_positive_finite(page_width)
        || !is_positive_finite(page_height)
    {
        return Err(DocumentError::InvalidImageDimensions);
    }

    let scale = (page_width / image_width).min(page_height / image_height);
    let width = image_width * scale;
    let height = image_height * scale;

    Ok(FitRect {
        x: (page_width - width) / 2.0,
        y: (page_height - height) / 2.0,
        width,
        height,
    })
}

/// Convert PNG or JPEG into a single-page PDF matching target paper dimensions.
pub fn image_to_pdf(
    image_path: &Path,
    paper: &EffectivePaper,
    output_path: &Path,
) -> DocumentResult<()> {
    let image_format = match detect_format(image_path)? {
        Some(DocumentFormat::Png) => ImageFormat::Png,
        Some(DocumentFormat::Jpeg) => ImageFormat::Jpeg,
        _ => return Err(DocumentError::UnsupportedFormat),
    };

    paper
        .validate()
        .map_err(|_| DocumentError::InvalidImageDimensions)?;

    let mut image_reader = ImageReader::open(image_path)?;
    image_reader.set_format(image_format);
    let image = image_reader.decode()?;
    let (image_width, image_height) = image.dimensions();
    if image_width == 0 || image_height == 0 {
        return Err(DocumentError::InvalidImageDimensions);
    }

    let fit = fit_contain(
        image_width as f64,
        image_height as f64,
        paper.width_mm,
        paper.height_mm,
    )?;
    let raw_image = raw_image_from_dynamic_image(image);

    // PDF print coordinates use physical units, so image pixels are first mapped to stable
    // label printer DPI, then scaled into the calculated layout rectangle.
    let mut doc = PdfDocument::new("print-bridge-document");
    let image_id = doc.add_image(&raw_image);
    let natural_width_mm = image_width as f64 * 25.4 / f64::from(DEFAULT_DPI);
    let natural_height_mm = image_height as f64 * 25.4 / f64::from(DEFAULT_DPI);

    let page = PdfPage::new(
        Mm(paper.width_mm as f32),
        Mm(paper.height_mm as f32),
        vec![Op::UseXobject {
            id: image_id,
            transform: XObjectTransform {
                translate_x: Some(mm_to_pt(fit.x)),
                translate_y: Some(mm_to_pt(fit.y)),
                rotate: None,
                scale_x: Some((fit.width / natural_width_mm) as f32),
                scale_y: Some((fit.height / natural_height_mm) as f32),
                dpi: Some(DEFAULT_DPI),
            },
        }],
    );

    let bytes = doc
        .with_pages(vec![page])
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    fs::write(output_path, bytes)?;

    Ok(())
}

/// Generate vector calibration test page for verifying label offset and scaling.
pub fn calibration_page_to_pdf(paper: &EffectivePaper, output_path: &Path) -> DocumentResult<()> {
    paper
        .validate()
        .map_err(|_| DocumentError::InvalidImageDimensions)?;
    if !is_positive_finite(paper.width_mm) || !is_positive_finite(paper.height_mm) {
        return Err(DocumentError::InvalidImageDimensions);
    }

    let mut doc = PdfDocument::new("print-bridge-calibration-page");
    let page = PdfPage::new(
        Mm(paper.width_mm as f32),
        Mm(paper.height_mm as f32),
        calibration_page_ops(paper),
    );
    let bytes = doc
        .with_pages(vec![page])
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    fs::write(output_path, bytes)?;

    Ok(())
}

/// Construct calibration page borders, centerlines, and dimension markers.
fn calibration_page_ops(paper: &EffectivePaper) -> Vec<Op> {
    let min_side = paper.width_mm.min(paper.height_mm);
    let margin = (min_side * 0.08).clamp(0.2, 4.0).min(min_side / 3.0);
    let width = paper.width_mm;
    let height = paper.height_mm;
    let mid_x = width / 2.0;
    let mid_y = height / 2.0;
    let text_size = (min_side * 0.13).clamp(1.0, 8.0);

    vec![
        Op::SaveGraphicsState,
        Op::SetOutlineColor {
            col: grayscale(0.0),
        },
        Op::SetOutlineThickness { pt: Pt(0.7) },
        Op::DrawLine {
            line: closed_line(&[
                (margin, margin),
                (width - margin, margin),
                (width - margin, height - margin),
                (margin, height - margin),
            ]),
        },
        Op::SetOutlineThickness { pt: Pt(0.35) },
        Op::SetOutlineColor {
            col: grayscale(0.35),
        },
        Op::DrawLine {
            line: open_line(&[(mid_x, margin), (mid_x, height - margin)]),
        },
        Op::DrawLine {
            line: open_line(&[(margin, mid_y), (width - margin, mid_y)]),
        },
        Op::StartTextSection,
        Op::SetFillColor {
            col: grayscale(0.0),
        },
        Op::SetFont {
            font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
            size: Pt(text_size as f32),
        },
        Op::SetTextCursor {
            pos: point_mm(margin, (height - margin - text_size * 0.35).max(margin)),
        },
        Op::ShowText {
            items: vec![TextItem::Text("PrintBridge Test".to_string())],
        },
        Op::SetTextCursor {
            pos: point_mm(margin, (margin + text_size * 0.2).min(height - margin)),
        },
        Op::ShowText {
            items: vec![TextItem::Text(format!(
                "{} x {} mm",
                format_mm_label(width),
                format_mm_label(height)
            ))],
        },
        Op::EndTextSection,
        Op::RestoreGraphicsState,
    ]
}

fn open_line(points: &[(f64, f64)]) -> Line {
    Line {
        points: points.iter().map(|(x, y)| line_point(*x, *y)).collect(),
        is_closed: false,
    }
}

fn closed_line(points: &[(f64, f64)]) -> Line {
    Line {
        points: points.iter().map(|(x, y)| line_point(*x, *y)).collect(),
        is_closed: true,
    }
}

fn line_point(x: f64, y: f64) -> LinePoint {
    LinePoint {
        p: point_mm(x, y),
        bezier: false,
    }
}

fn point_mm(x: f64, y: f64) -> Point {
    Point {
        x: mm_to_pt(x),
        y: mm_to_pt(y),
    }
}

fn grayscale(value: f32) -> Color {
    Color::Rgb(PdfRgb {
        r: value,
        g: value,
        b: value,
        icc_profile: None,
    })
}

fn format_mm_label(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// Convert millimeters to PDF points required by printpdf transformations.
fn mm_to_pt(value: f64) -> Pt {
    Mm(value as f32).into()
}

/// Convert image into raw RGB format required by printpdf.
fn raw_image_from_dynamic_image(image: DynamicImage) -> RawImage {
    let (width, height, pixels) = flatten_to_white(image);

    RawImage {
        pixels: RawImageData::U8(pixels),
        width: width as usize,
        height: height as usize,
        data_format: RawImageFormat::RGB8,
        tag: Vec::new(),
    }
}

/// Composite alpha transparency over white background to ensure predictable label output.
fn flatten_to_white(image: DynamicImage) -> (u32, u32, Vec<u8>) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = ImageBuffer::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = f32::from(pixel[3]) / 255.0;
        let red = blend_channel(pixel[0], alpha);
        let green = blend_channel(pixel[1], alpha);
        let blue = blend_channel(pixel[2], alpha);
        rgb.put_pixel(x, y, Rgb([red, green, blue]));
    }

    (width, height, rgb.into_raw())
}

/// Blend an individual color channel with white background.
fn blend_channel(channel: u8, alpha: f32) -> u8 {
    (f32::from(channel) * alpha + 255.0 * (1.0 - alpha)).round() as u8
}

/// Check whether dimensions can safely participate in layout calculations.
fn is_positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

#[cfg(test)]
mod tests {
    use super::{calibration_page_to_pdf, EffectivePaper, PdfDocument};
    use printpdf::PdfParseOptions;

    #[test]
    fn calibration_pdf_preserves_requested_paper_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("calibration.pdf");

        for (width_mm, height_mm) in [(230.0, 140.0), (140.0, 230.0), (50.0, 50.0), (210.0, 297.0)]
        {
            let paper = EffectivePaper {
                width_mm,
                height_mm,
            };
            calibration_page_to_pdf(&paper, &path).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let doc =
                PdfDocument::parse(&bytes, &PdfParseOptions::default(), &mut Vec::new()).unwrap();

            assert_eq!(doc.pages.len(), 1);
            let page = &doc.pages[0];
            // SumatraPDF uses MediaBox for paper selection; actual dimensions must be retained instead of just printing dimension text.
            // printpdf rounds MediaBox to integer pt, allowing up to 0.5 pt serialization tolerance.
            assert!(
                (f64::from(page.media_box.width.0) - width_mm * 72.0 / 25.4).abs() < 0.51,
                "width: {} pt for {width_mm} mm",
                page.media_box.width.0
            );
            assert!(
                (f64::from(page.media_box.height.0) - height_mm * 72.0 / 25.4).abs() < 0.51,
                "height: {} pt for {height_mm} mm",
                page.media_box.height.0
            );
        }
    }
}
