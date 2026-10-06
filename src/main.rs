//! ImageMin 的桌面界面和程序入口。
//!
//! GPUI 使用声明式的 builder API 构建界面：`div().尺寸().颜色().child(...)` 每一步
//! 返回可继续配置的元素。Rust 会在编译期检查这些方法和事件回调的类型。

// `std` 是标准库，`::` 用来逐级访问模块或类型中的成员。
use std::path::PathBuf;

// 花括号可以从同一路径一次导入多个名字；嵌套花括号表示子模块。
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, TitleBar,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
// `*` 是通配导入。GUI prelude 提供很多常用类型；业务模块通常更适合显式导入。
use gpui_kit::*;
// `self` 导入 optimizer 模块本身，同时导入其中的 OptimizationResult 类型。
use image_min::optimizer::{self, OptimizationResult};

/// 单张图片当前所处的状态。
///
/// Rust 的 `enum` 是“带数据的枚举”：`Optimizing` 不带数据，`Complete(...)` 保存成功
/// 结果，`Failed(...)` 保存一个拥有所有权的错误字符串。
#[derive(Clone)]
enum ItemState {
    Optimizing,
    Complete(OptimizationResult),
    Failed(String),
}

/// 列表中的一张图片。
#[derive(Clone)]
struct ImageItem {
    // 每个字段写作 `名称: 类型`。u64 是可复制的整数；PathBuf 和 ItemState 拥有数据。
    id: u64,
    path: PathBuf,
    original_bytes: u64,
    state: ItemState,
}

/// 整个窗口的可变状态。UI 重绘时会读取这里的数据。
struct ImageMinApp {
    // `Vec<T>` 是可增长数组；尖括号中的 `T` 是泛型参数。
    items: Vec<ImageItem>,
    next_id: u64,
}

// `impl ImageMinApp` 中定义该类型自己的方法。`Self` 在这里就是 ImageMinApp。
impl ImageMinApp {
    /// 创建初始状态。关联函数没有 self 参数，使用 `ImageMinApp::new()` 调用。
    fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }

    /// 打开系统文件选择器。
    ///
    /// `&mut self` 是对应用状态的独占可变借用；`&mut Window` 和 `&mut Context<Self>`
    /// 同理，保证回调执行时不会有两个地方同时修改这些值。
    fn choose_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            // Option 用 Some(value) 表示有值、None 表示没有值。`into()` 按目标类型转换。
            prompt: Some("Choose images to optimize".into()),
        });

        // `async move` 创建异步闭包。`move` 把 selection 的所有权移入闭包，保证当前
        // 方法返回后异步任务仍能安全使用它。
        cx.spawn_in(window, async move |this, window| {
            // `await` 等待文件选择但不阻塞 UI 线程。这里有多层 Result/Option：
            // `ok()` 先把 Err 转成 None，后续每个 `?` 遇到 None（失败或取消）便提前结束任务。
            let paths = selection.await.ok()?.ok()??;
            // 异步完成后通过 update_in 回到 GPUI 上下文修改界面状态。
            this.update_in(window, |this, _, cx| this.add_paths(paths, cx))
                .ok()
        })
        // detach 让任务独立运行；若不 detach 或保存任务句柄，句柄被丢弃会取消任务。
        .detach();
    }

    /// 过滤并加入用户选择或拖入的路径，然后为每张图启动压缩任务。
    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // `for path in paths` 会取得 Vec 所有权，并逐个把 PathBuf 移入 path。
        for path in paths {
            // `||` 是短路“或”：前一条件为 true 时不会计算后一条件。
            if !path.is_file()
                || !optimizer::is_supported(&path)
                // iter() 只读借用元素；any 接收闭包并判断是否至少一个元素满足条件。
                || self.items.iter().any(|item| item.path == path)
            {
                // continue 跳过当前循环，其余路径继续处理。
                continue;
            }

            // map 转换成功的 metadata；unwrap_or 在读取失败时使用 0。
            let original_bytes = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            let id = self.next_id;
            self.next_id += 1;
            self.items.push(ImageItem {
                id,
                // 列表和后台任务都需要拥有路径，因此此处克隆一份 PathBuf。
                path: path.clone(),
                original_bytes,
                state: ItemState::Optimizing,
            });
            self.optimize_image(id, path, cx);
        }
        // 通知 GPUI 状态已变化，需要重新调用 render。
        cx.notify();
    }

    /// 在后台线程池执行 CPU/磁盘密集的压缩，避免阻塞窗口交互。
    fn optimize_image(&self, id: u64, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                // 内层 move 闭包取得 path 所有权；&path 只在 optimize 调用期间借用它。
                .background_spawn(async move { optimizer::optimize(&path) })
                .await;

            // 修改实体必须回到 GPUI 的 update 闭包。
            this.update(cx, |this, cx| {
                // iter_mut() 产生可变引用；find 返回 Option<&mut ImageItem>。
                if let Some(item) = this.items.iter_mut().find(|item| item.id == id) {
                    // match 解构 Result，并把成功值或错误文字放入对应枚举变体。
                    item.state = match result {
                        Ok(result) => ItemState::Complete(result),
                        Err(error) => ItemState::Failed(error.to_string()),
                    };
                }
                cx.notify();
            })
            // `.ok()` 明确丢弃 UI 实体已关闭时可能产生的更新错误。
            .ok();
        })
        .detach();
    }

    fn clear_finished(&mut self, cx: &mut Context<Self>) {
        // retain 只保留闭包返回 true 的元素。matches! 在此只匹配正在压缩的状态。
        self.items
            .retain(|item| matches!(item.state, ItemState::Optimizing));
        cx.notify();
    }

    // `impl IntoElement` 表示“返回某个能转换为元素的具体类型”，调用者无需知道长类型名。
    fn render_summary(&self, cx: &Context<Self>) -> impl IntoElement {
        let complete = self
            .items
            .iter()
            // filter_map 同时完成过滤和映射；`&item.state` 避免移走枚举中的结果。
            .filter_map(|item| match &item.state {
                ItemState::Complete(result) => Some(result),
                // `_` 是通配模式，代表其他所有状态且不绑定变量。
                _ => None,
            })
            // `::<Vec<_>>` 是 turbofish 语法：指定收集为 Vec，`_` 让编译器推导元素类型。
            .collect::<Vec<_>>();
        let saved = complete
            .iter()
            .map(|result| result.bytes_saved())
            // sum 也是泛型方法，这里明确要求结果类型为 u64。
            .sum::<u64>();
        let original = complete
            .iter()
            .map(|result| result.original_bytes)
            .sum::<u64>();
        // Rust 的 if 是表达式，所以可以直接把某个分支产生的值赋给变量。
        let percent = if original == 0 {
            0.0
        } else {
            saved as f64 / original as f64 * 100.0
        };

        h_flex()
            .gap_6()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(format!("{} images", self.items.len()))
            .child(format!("{} saved", format_bytes(saved)))
            // `{percent:.1}` 表示保留一位小数。
            .child(format!("{percent:.1}% smaller"))
    }

    /// 渲染一行。`AnyElement` 用类型擦除统一不同 builder 组合产生的复杂类型。
    fn render_item(item: &ImageItem, cx: &Context<Self>) -> AnyElement {
        let name = item
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unnamed image")
            // to_owned 把借用的 &str 复制为拥有所有权的 String，供元素长期保存。
            .to_owned();
        // 元组模式 `(a, b, c)` 一次解构三个返回值。
        let (status, detail, color) = match &item.state {
            ItemState::Optimizing => (
                "Optimizing…".to_owned(),
                format_bytes(item.original_bytes),
                cx.theme().blue,
            ),
            ItemState::Complete(result) => (
                format!("−{:.1}%", result.percent_saved()),
                format!(
                    "{} → {}  •  {}",
                    format_bytes(result.original_bytes),
                    format_bytes(result.optimized_bytes),
                    result
                        .output_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("optimized image")
                ),
                cx.theme().green,
            ),
            ItemState::Failed(error) => ("Failed".to_owned(), error.clone(), cx.theme().red),
        };

        // 以下是 builder 链：每个方法消费/返回 builder，最后构成一棵 UI 元素树。
        h_flex()
            .w_full()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size_10()
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_lg()
                    .child("▧"),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(div().text_sm().font_medium().truncate().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .text_color(color)
                    .child(status),
            )
            // 转成统一的 AnyElement 返回类型。
            .into_any_element()
    }
}

// `Render` 是 GPUI 定义的 trait（类似接口）。实现它后，ImageMinApp 才能作为视图绘制。
impl Render for ImageMinApp {
    // `_window` 的下划线前缀表示参数目前有意不使用，编译器不会发出警告。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_finished = self
            .items
            .iter()
            .any(|item| !matches!(item.state, ItemState::Optimizing));

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .w_full()
                        .pr_3()
                        .justify_between()
                        .child(div().font_semibold().child("ImageMin"))
                        .child(
                            Button::new("add-title")
                                .primary()
                                .label("Add images")
                                // listener 把闭包注册为事件回调；第二个 `_` 忽略点击事件数据。
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.choose_images(window, cx);
                                })),
                        ),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_5()
                    .gap_4()
                    .child(
                        div()
                            .id("drop-zone")
                            .w_full()
                            .p_6()
                            .rounded_xl()
                            .border_2()
                            .border_dashed()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted.opacity(0.25))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            // `is::<ExternalPaths>()` 用 turbofish 指定允许拖入的值类型。
                            .can_drop(|value, _, _| value.is::<ExternalPaths>())
                            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                                this.add_paths(paths.paths().to_vec(), cx);
                            }))
                            .child(div().text_2xl().child("⇩"))
                            .child(div().font_semibold().child("Drop images here"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("PNG, JPEG, or WebP • originals are never overwritten"),
                            )
                            .child(
                                Button::new("choose-images")
                                    .secondary()
                                    .label("Choose files")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_images(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .child(self.render_summary(cx))
                            .child(
                                Button::new("clear-finished")
                                    .ghost()
                                    .label("Clear finished")
                                    .disabled(!has_finished)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_finished(cx);
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("image-list")
                            .flex_1()
                            .min_h(px(160.))
                            .overflow_y_scroll()
                            .rounded_xl()
                            .border_1()
                            .border_color(cx.theme().border)
                            // when 仅在条件成立时应用闭包，构建空列表提示。
                            .when(self.items.is_empty(), |list| {
                                list.child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Optimized images will appear here"),
                                )
                            })
                            // Self:: 调用当前类型的关联函数；map 为每个数据项创建一行元素。
                            .children(self.items.iter().map(|item| Self::render_item(item, cx))),
                    ),
            )
    }
}

fn format_bytes(bytes: u64) -> String {
    // 函数内部也能定义常量，作用域仅限本函数。
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    // Rust 允许用 let “遮蔽”同名变量；新 bytes 是 f64，旧的 u64 此后不可见。
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// 可执行程序入口。Rust 启动时从 `main` 开始执行。
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(760.), px(680.)), cx)),
                // `..value` 是结构体更新语法：未显式填写的字段取自这个默认配置。
                ..TitleBar::window_options()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                window.activate_window();
                window.set_window_title("ImageMin");
                // `|_|` 是不使用参数的闭包；它创建并交回应用状态实体。
                cx.new(|_| ImageMinApp::new())
            })
            // expect 在 Err 时终止程序并显示这段上下文；启动窗口失败时无法继续运行。
            .expect("failed to open ImageMin window");
        });
}
