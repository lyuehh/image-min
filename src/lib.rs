//! ImageMin 的库入口（library crate root）。
//!
//! Rust 项目可以同时包含“库”和“可执行程序”：本文件编译为库，`main.rs` 编译为
//! 可执行程序。把图片压缩逻辑放在库里，既方便界面调用，也方便单元测试复用。

/// `pub mod` 声明一个公开模块；编译器会加载同目录的 `optimizer.rs`。
///
/// `pub` 表示其他 crate 也能访问它，所以 `main.rs` 才能写
/// `image_min::optimizer`。这里的 `::` 是路径分隔符，类似文件路径中的 `/`。
pub mod optimizer;
