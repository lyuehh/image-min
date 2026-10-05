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

## macOS 打包

在 macOS 上运行：

```bash
./scripts/package-macos.sh
open dist/ImageMin.app
```

脚本会编译 release 版本，在 `dist/ImageMin.app` 生成可双击打开的应用，并进行本机 ad-hoc 签名。若要使用 Apple Developer 证书签名以供其他用户分发，可指定签名身份：

```bash
MACOS_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
  ./scripts/package-macos.sh
```

推送 `v*` 标签（例如 `v0.1.0`）会触发 GitHub Actions，分别生成 Apple Silicon 和 Intel Mac 的 zip 包并附加到 GitHub Release。公开分发仍建议配置 Developer ID 证书并完成 Apple notarization；默认 CI 产物采用 ad-hoc 签名，首次打开时可能需要在系统设置的“隐私与安全性”中确认。

## 验证

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
