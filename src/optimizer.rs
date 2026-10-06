//! 与界面无关的图片压缩逻辑。
//!
//! 本模块展示了 Rust 中常见的错误处理、借用、切片、枚举匹配和文件操作。函数只接收
//! 图片路径并返回结果，不依赖 GPUI，因此可以独立测试。

// `use` 把较长的路径引入当前作用域。花括号表示一次导入多个成员。
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, codecs::jpeg::JpegEncoder};

// `const` 是编译期常量，必须写明类型。`u8` 是 0..=255 的无符号整数。
const JPEG_QUALITY: u8 = 82;

/// 一次压缩的输出信息。
///
/// `#[derive(...)]` 是属性（attribute）：让编译器自动生成调试输出、克隆和比较所需的
/// trait 实现。`PathBuf` 和 `u64` 字段由结构体拥有，不会借用调用者的临时数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimizationResult {
    /// 生成文件的完整路径。`PathBuf` 是拥有所有权、可增长的路径类型。
    pub output_path: PathBuf,
    /// 原文件字节数。`u64` 可表达较大的文件长度。
    pub original_bytes: u64,
    /// 输出文件字节数。
    pub optimized_bytes: u64,
}

// `impl Type` 为类型定义关联函数或方法。第一个参数为 `&self` 时，它是只读借用方法：
// 调用期间可以读当前值，但不能修改或取得其所有权。
impl OptimizationResult {
    /// 返回节省的字节数。
    pub fn bytes_saved(&self) -> u64 {
        // 普通无符号减法在结果小于 0 时会溢出；`saturating_sub` 会安全地停在 0。
        self.original_bytes.saturating_sub(self.optimized_bytes)
    }

    /// 返回 0 到 100 之间的压缩百分比（输出未变小时为 0）。
    pub fn percent_saved(&self) -> f64 {
        if self.original_bytes == 0 {
            // `return` 提前结束函数；末尾分号表示这是语句，不把值继续传给后续表达式。
            return 0.0;
        }
        // `as f64` 做显式数值转换。Rust 不会自动把整数转换为浮点数。
        // 函数最后一个没有分号的表达式就是返回值，因此这里不需要写 `return`。
        self.bytes_saved() as f64 / self.original_bytes as f64 * 100.0
    }
}

/// 判断路径的扩展名是否受支持。
///
/// `&Path` 是对路径的只读借用；与接收 `PathBuf` 相比，它不复制路径，也不取得所有权。
pub fn is_supported(path: &Path) -> bool {
    // `matches!` 是宏（`!` 表示宏调用），用于判断一个值是否匹配给定模式。
    matches!(
        path.extension()
            // `extension()` 返回 Option；`and_then` 只在它为 Some 时继续转换为 UTF-8。
            .and_then(|extension| extension.to_str())
            // `|参数| 表达式` 是闭包。这里把扩展名转成小写，并产生一个新 String。
            .map(str::to_ascii_lowercase)
            // `as_deref()` 把 Option<String> 临时借用成 Option<&str>，便于匹配字符串字面量。
            .as_deref(),
        // 模式中的 `|` 表示“或”，不是对字符串执行位运算。
        Some("jpg" | "jpeg" | "png" | "webp")
    )
}

/// 根据原路径生成不覆盖原文件的输出路径，例如 `photo.jpg` → `photo.min.jpg`。
///
/// `Result<PathBuf>` 是 `anyhow::Result<PathBuf>` 的简写：成功时是 `Ok(PathBuf)`，失败时
/// 是 `Err(error)`。
pub fn optimized_path(source: &Path) -> Result<PathBuf> {
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        // `context` 把 Option 的 None 转成带说明的错误；`?` 遇到错误便立即返回。
        .context("image has no valid file name")?;
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .context("image has no valid extension")?;
    // `format!` 支持在 `{name}` 中直接引用同名变量。`Ok(...)` 包装成功值。
    Ok(source.with_file_name(format!("{stem}.min.{extension}")))
}

/// 压缩一张图片并在原图旁写入新文件。
pub fn optimize(source: &Path) -> Result<OptimizationResult> {
    if !is_supported(source) {
        // 前缀 `!` 对布尔值取反；`bail!` 构造错误并立即从当前函数返回。
        bail!("unsupported image type (use PNG, JPEG, or WebP)");
    }

    // `with_context` 接收闭包，只有发生错误时才创建包含实际路径的错误消息。
    let original =
        fs::read(source).with_context(|| format!("could not read {}", source.display()))?;
    let format = ImageFormat::from_path(source).context("could not determine image format")?;

    // `match` 必须覆盖枚举的所有可能值。每个分支都会产生同一种类型 `Vec<u8>`。
    let candidate = match format {
        // `&original` 借用 Vec；`?` 会取出 Ok 中的数据，或把 Err 向上传播。
        ImageFormat::Png => optimize_png(&original)?,
        ImageFormat::Jpeg => optimize_jpeg(&original)?,
        ImageFormat::WebP => optimize_webp(&original)?,
        _ => bail!("unsupported image type (use PNG, JPEG, or WebP)"),
    };

    // 重编码已经优化过的文件有时反而会变大，因此仅在候选结果更小时采用它。
    let optimized = if candidate.len() < original.len() {
        // 这里把 candidate 移入 optimized；之后不能再使用 candidate。
        candidate
    } else {
        // `clone` 复制 Vec 及其字节。必须复制是因为稍后仍要读取 original.len()。
        original.clone()
    };
    let output_path = optimized_path(source)?;
    write_atomically(&output_path, &optimized)?;

    // 结构体字面量逐字段创建返回值。结尾没有分号，因此它是函数返回表达式。
    Ok(OptimizationResult {
        output_path,
        original_bytes: original.len() as u64,
        optimized_bytes: optimized.len() as u64,
    })
}

// `&[u8]` 是只读字节切片：包含地址和长度，但不拥有底层数据。
// `Vec<u8>` 则拥有返回数据，调用者可在输入借用结束后继续使用它。
fn optimize_png(input: &[u8]) -> Result<Vec<u8>> {
    let options = oxipng::Options::from_preset(3);
    oxipng::optimize_from_memory(input, &options).context("PNG optimization failed")
}

fn optimize_jpeg(input: &[u8]) -> Result<Vec<u8>> {
    let image =
        decode_with_orientation(input, ImageFormat::Jpeg).context("JPEG decoding failed")?;
    // 变量默认不可修改；`mut` 允许编码器向 output 追加字节。
    let mut output = Vec::new();
    // `&mut output` 是独占可变借用。同一时刻不能再通过其他引用修改 output。
    JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
        .encode_image(&image)
        .context("JPEG encoding failed")?;
    Ok(output)
}

fn optimize_webp(input: &[u8]) -> Result<Vec<u8>> {
    let image =
        decode_with_orientation(input, ImageFormat::WebP).context("WebP decoding failed")?;
    // Cursor 为内存中的 Vec 提供类似文件的 Seek/Write 接口。
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::WebP)
        .context("WebP encoding failed")?;
    // `into_inner(self)` 消耗 Cursor 并取回其拥有的 Vec，因此没有额外复制。
    Ok(output.into_inner())
}

/// 解码图片，并按照 EXIF 等元数据中的方向旋转/翻转像素。
fn decode_with_orientation(input: &[u8], format: ImageFormat) -> Result<DynamicImage> {
    let mut decoder = ImageReader::with_format(Cursor::new(input), format).into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// 先写临时文件再重命名，避免写入中途失败而留下半个输出文件。
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
    // `if let` 适合只处理一种模式：这里成功时不做事，只处理 Err(error)。
    if let Err(error) = fs::rename(&temporary, destination) {
        // `let _ =` 明确忽略清理失败；不能让次要错误盖过真正的重命名错误。
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("could not save {}", destination.display()));
    }
    Ok(())
}

// `cfg(test)` 表示这个模块只在 `cargo test` 编译时存在，不会进入正式程序。
#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use image::{DynamicImage, RgbImage};

    // `super` 指父模块；`*` 导入其中所有公开给子模块的名字。
    use super::*;

    fn temporary_directory() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            // 测试中使用 unwrap 可以接受：失败会立即让该测试失败并显示位置；
            // 正式业务代码应优先返回 Result 或提供明确错误信息。
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("image-min-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    // `#[test]` 注册测试函数；`assert!`/`assert_eq!` 条件不满足时会 panic。
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
