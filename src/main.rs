use std::path::PathBuf;

use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, TitleBar,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use image_min::optimizer::{self, OptimizationResult};

#[derive(Clone)]
enum ItemState {
    Optimizing,
    Complete(OptimizationResult),
    Failed(String),
}

#[derive(Clone)]
struct ImageItem {
    id: u64,
    path: PathBuf,
    original_bytes: u64,
    state: ItemState,
}

struct ImageMinApp {
    items: Vec<ImageItem>,
    next_id: u64,
}

impl ImageMinApp {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }

    fn choose_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Choose images to optimize".into()),
        });

        cx.spawn_in(window, async move |this, window| {
            let paths = selection.await.ok()?.ok()??;
            this.update_in(window, |this, _, cx| this.add_paths(paths, cx))
                .ok()
        })
        .detach();
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            if !path.is_file()
                || !optimizer::is_supported(&path)
                || self.items.iter().any(|item| item.path == path)
            {
                continue;
            }

            let original_bytes = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            let id = self.next_id;
            self.next_id += 1;
            self.items.push(ImageItem {
                id,
                path: path.clone(),
                original_bytes,
                state: ItemState::Optimizing,
            });
            self.optimize_image(id, path, cx);
        }
        cx.notify();
    }

    fn optimize_image(&self, id: u64, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { optimizer::optimize(&path) })
                .await;

            this.update(cx, |this, cx| {
                if let Some(item) = this.items.iter_mut().find(|item| item.id == id) {
                    item.state = match result {
                        Ok(result) => ItemState::Complete(result),
                        Err(error) => ItemState::Failed(error.to_string()),
                    };
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.items
            .retain(|item| matches!(item.state, ItemState::Optimizing));
        cx.notify();
    }

    fn render_summary(&self, cx: &Context<Self>) -> impl IntoElement {
        let complete = self
            .items
            .iter()
            .filter_map(|item| match &item.state {
                ItemState::Complete(result) => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        let saved = complete
            .iter()
            .map(|result| result.bytes_saved())
            .sum::<u64>();
        let original = complete
            .iter()
            .map(|result| result.original_bytes)
            .sum::<u64>();
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
            .child(format!("{percent:.1}% smaller"))
    }

    fn render_item(item: &ImageItem, cx: &Context<Self>) -> AnyElement {
        let name = item
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unnamed image")
            .to_owned();
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
            .into_any_element()
    }
}

impl Render for ImageMinApp {
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
                            .children(self.items.iter().map(|item| Self::render_item(item, cx))),
                    ),
            )
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(760.), px(680.)), cx)),
                ..TitleBar::window_options()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                window.activate_window();
                window.set_window_title("ImageMin");
                cx.new(|_| ImageMinApp::new())
            })
            .expect("failed to open ImageMin window");
        });
}
