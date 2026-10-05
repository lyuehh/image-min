# ImageMin

一款使用 Rust 和 [GPUI Kit](https://gpui-kit.com/) 构建的原生图片压缩工具，交互方式参考 ImageOptim。

## 功能

- 拖拽或批量选择 PNG、JPEG、WebP 图片
- 后台压缩，不阻塞界面
- 实时显示压缩前后大小和节省比例
- 在原图旁生成 `*.min.png` / `*.min.jpg` / `*.min.webp`，不会覆盖原文件
- 当重新编码无法减小文件时保留原始字节，保证输出不会变大

## 运行

需要 Rust stable 和 GPUI Kit 对应平台的系统依赖。

```bash
cargo run --release
```

## 验证

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
