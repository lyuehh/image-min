use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, codecs::jpeg::JpegEncoder};

const JPEG_QUALITY: u8 = 82;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimizationResult {
    pub output_path: PathBuf,
    pub original_bytes: u64,
    pub optimized_bytes: u64,
}

impl OptimizationResult {
    pub fn bytes_saved(&self) -> u64 {
        self.original_bytes.saturating_sub(self.optimized_bytes)
    }

    pub fn percent_saved(&self) -> f64 {
        if self.original_bytes == 0 {
            return 0.0;
        }
        self.bytes_saved() as f64 / self.original_bytes as f64 * 100.0
    }
}

pub fn is_supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp")
    )
}

pub fn optimized_path(source: &Path) -> Result<PathBuf> {
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("image has no valid file name")?;
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .context("image has no valid extension")?;
    Ok(source.with_file_name(format!("{stem}.min.{extension}")))
}

pub fn optimize(source: &Path) -> Result<OptimizationResult> {
    if !is_supported(source) {
        bail!("unsupported image type (use PNG, JPEG, or WebP)");
    }

    let original =
        fs::read(source).with_context(|| format!("could not read {}", source.display()))?;
    let format = ImageFormat::from_path(source).context("could not determine image format")?;

    let candidate = match format {
        ImageFormat::Png => optimize_png(&original)?,
        ImageFormat::Jpeg => optimize_jpeg(&original)?,
        ImageFormat::WebP => optimize_webp(&original)?,
        _ => bail!("unsupported image type (use PNG, JPEG, or WebP)"),
    };

    // Re-encoding can occasionally grow an already optimized file. In that case,
    // preserve the original bytes so the optimizer never makes an image larger.
    let optimized = if candidate.len() < original.len() {
        candidate
    } else {
        original.clone()
    };
    let output_path = optimized_path(source)?;
    write_atomically(&output_path, &optimized)?;

    Ok(OptimizationResult {
        output_path,
        original_bytes: original.len() as u64,
        optimized_bytes: optimized.len() as u64,
    })
}

fn optimize_png(input: &[u8]) -> Result<Vec<u8>> {
    let options = oxipng::Options::from_preset(3);
    oxipng::optimize_from_memory(input, &options).context("PNG optimization failed")
}

fn optimize_jpeg(input: &[u8]) -> Result<Vec<u8>> {
    let image =
        decode_with_orientation(input, ImageFormat::Jpeg).context("JPEG decoding failed")?;
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
        .encode_image(&image)
        .context("JPEG encoding failed")?;
    Ok(output)
}

fn optimize_webp(input: &[u8]) -> Result<Vec<u8>> {
    let image =
        decode_with_orientation(input, ImageFormat::WebP).context("WebP decoding failed")?;
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::WebP)
        .context("WebP encoding failed")?;
    Ok(output.into_inner())
}

fn decode_with_orientation(input: &[u8], format: ImageFormat) -> Result<DynamicImage> {
    let mut decoder = ImageReader::with_format(Cursor::new(input), format).into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn write_atomically(destination: &Path, contents: &[u8]) -> Result<()> {
    let temporary = destination.with_extension(format!(
        "{}.tmp",
        destination
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("image")
    ));
    fs::write(&temporary, contents)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("could not save {}", destination.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use image::{DynamicImage, RgbImage};

    use super::*;

    fn temporary_directory() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("image-min-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn recognizes_supported_extensions_case_insensitively() {
        assert!(is_supported(Path::new("photo.JPG")));
        assert!(is_supported(Path::new("graphic.png")));
        assert!(is_supported(Path::new("image.webp")));
        assert!(!is_supported(Path::new("animation.gif")));
    }

    #[test]
    fn creates_a_non_destructive_output_name() {
        assert_eq!(
            optimized_path(Path::new("/photos/holiday.jpeg")).unwrap(),
            PathBuf::from("/photos/holiday.min.jpeg")
        );
    }

    #[test]
    fn optimizes_a_png_without_changing_its_dimensions() {
        let directory = temporary_directory();
        let source = directory.join("sample.png");
        let image = RgbImage::from_fn(48, 32, |x, y| {
            image::Rgb([(x % 8) as u8 * 24, (y % 8) as u8 * 24, 120])
        });
        DynamicImage::ImageRgb8(image).save(&source).unwrap();

        let result = optimize(&source).unwrap();

        assert_eq!(
            image::image_dimensions(&result.output_path).unwrap(),
            (48, 32)
        );
        assert!(result.optimized_bytes <= result.original_bytes);
        assert!(result.output_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_an_unsupported_file_without_creating_output() {
        let directory = temporary_directory();
        let source = directory.join("notes.txt");
        fs::write(&source, "not an image").unwrap();

        let error = optimize(&source).unwrap_err().to_string();

        assert!(error.contains("unsupported image type"));
        assert!(!directory.join("notes.min.txt").exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
