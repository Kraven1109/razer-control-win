/// Shared UI widget helpers and global style setup.

use crate::app::App;
use crate::constants::*;
use eframe::egui::{self, pos2, vec2, Color32, FontId, RichText, Stroke, Ui};

use crate::app::{Banner, BannerTone};

// ── Layout primitives ─────────────────────────────────────────────────────────

pub fn card(ui: &mut Ui, title: &str, subtitle: &str, add_body: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    card_inner(ui, title, subtitle, add_body);
}

/// Same as `card` but without the leading `add_space(10)` — use inside
/// `horizontal_top` layouts where the parent controls vertical spacing.
pub fn card_inner(ui: &mut Ui, title: &str, subtitle: &str, add_body: impl FnOnce(&mut Ui)) {
    egui::Frame {
        fill: CARD,
        rounding: egui::Rounding::same(16.0),
        stroke: Stroke::new(1.0, BORDER),
        inner_margin: egui::Margin::ZERO,
        outer_margin: egui::Margin::ZERO,
        ..Default::default()
    }
    .show(ui, |ui| {
        egui::Frame::none()
            .fill(CARD_ALT)
            .rounding(egui::Rounding {
                nw: 16.0,
                ne: 16.0,
                sw: 0.0,
                se: 0.0,
            })
            .inner_margin(egui::Margin {
                left: 18.0,
                right: 18.0,
                top: 14.0,
                bottom: 12.0,
            })
            .show(ui, |ui| {
                ui.label(RichText::new(title).size(14.5).strong().color(TEXT));
                if !subtitle.is_empty() {
                    ui.add_space(2.0);
                    ui.label(RichText::new(subtitle).size(11.5).color(DIM));
                }
            });

        egui::Frame::none()
            .inner_margin(egui::Margin {
                left: 18.0,
                right: 18.0,
                top: 12.0,
                bottom: 16.0,
            })
            .show(ui, add_body);
    });
}

pub fn row(ui: &mut Ui, title: &str, subtitle: &str, add_control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        vec2(width, 0.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                vec2(width * 0.58, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.label(RichText::new(title).size(13.8).color(TEXT));
                    if !subtitle.is_empty() {
                        ui.label(RichText::new(subtitle).size(11.2).color(DIM));
                    }
                },
            );
            let remaining = ui.available_width();
            ui.allocate_ui_with_layout(
                vec2(remaining, 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| add_control(ui),
            );
        },
    );
    ui.add_space(4.0);
}

pub fn rowsep(ui: &mut Ui) {
    ui.add_space(3.0);
    ui.add(egui::Separator::default().spacing(5.0));
    ui.add_space(3.0);
}

pub fn chip(ui: &mut Ui, text: &str, fill: Color32, stroke: Color32, text_color: Color32) {
    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .rounding(egui::Rounding::same(999.0))
        .inner_margin(egui::Margin {
            left: 10.0,
            right: 10.0,
            top: 5.0,
            bottom: 5.0,
        })
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(text_color));
        });
}

pub fn metric_tile(
    ui: &mut Ui,
    title: &str,
    value: impl Into<String>,
    subtitle: impl Into<String>,
    accent: Color32,
) {
    let value = value.into();
    let subtitle = subtitle.into();
    egui::Frame::none()
        .fill(CARD_ALT)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(14.0))
        .inner_margin(egui::Margin {
            left: 14.0,
            right: 14.0,
            top: 12.0,
            bottom: 12.0,
        })
        .show(ui, |ui| {
            ui.label(RichText::new(title).size(11.0).color(DIM));
            ui.add_space(6.0);
            ui.label(RichText::new(value).size(24.0).strong().color(accent));
            ui.add_space(4.0);
            ui.label(RichText::new(subtitle).size(11.0).color(SOFT));
        });
}

/// Two-metric tile — shows two values side-by-side inside one card,
/// useful for combining related metrics (e.g. GPU% + Temp, TGP + Clock).
// ── Page-level draw functions ─────────────────────────────────────────────────
pub fn draw_page_header(ui: &mut Ui, title: &str, subtitle: &str, app: &App) {
    let gpu_name = app.gpu.as_ref().map(|g| g.name.as_str());
    egui::Frame::none()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(20.0))
        .inner_margin(egui::Margin {
            left: 22.0,
            right: 22.0,
            top: 20.0,
            bottom: 16.0,
        })
        .show(ui, |ui| {
            // Decorative circles — painted first (behind content).
            // Clip to the outer card rect (inner + inner_margin) so they never
            // bleed into the window outside the rounded header card.
            {
                let inner = ui.max_rect();
                // Reconstruct outer from inner + known inner_margin values.
                let outer = egui::Rect::from_min_max(
                    pos2(inner.left() - 22.0, inner.top() - 20.0),
                    pos2(inner.right() + 22.0, inner.bottom() + 16.0),
                );
                let p = ui.painter().with_clip_rect(outer);
                p.circle_filled(
                    pos2(outer.right() - 62.0, outer.top() + 24.0),
                    68.0,
                    Color32::from_rgba_unmultiplied(68, 255, 161, 18),
                );
                p.circle_filled(
                    pos2(outer.right() - 16.0, outer.top() + 74.0),
                    52.0,
                    Color32::from_rgba_unmultiplied(84, 186, 255, 14),
                );
            }

            ui.label(RichText::new(title).size(24.0).strong().color(TEXT));
            ui.add_space(2.0);
            ui.label(RichText::new(subtitle).size(12.0).color(DIM));
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let (status_text, sc) =
                    if app.ok { ("Daemon online", OK) } else { ("Daemon offline", ERR) };
                chip(
                    ui,
                    status_text,
                    Color32::from_rgba_unmultiplied(sc.r(), sc.g(), sc.b(), 50),
                    sc,
                    sc,
                );
                chip(
                    ui,
                    &format!("Device: {}", app.devname),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                    BORDER,
                    TEXT,
                );
                let gpu_label = match gpu_name {
                    Some(n)
                        if n.contains("Task Manager")
                            || n.contains("PDH") =>
                    {
                        "GPU: PDH counters"
                    }
                    Some(_) => "GPU: live",
                    None => "GPU: N/A",
                };
                chip(
                    ui,
                    gpu_label,
                    Color32::from_rgba_unmultiplied(84, 186, 255, 32),
                    ACCENT_2,
                    ACCENT_2,
                );
            });
        });
    ui.add_space(4.0);
}

pub fn draw_banner(ui: &mut Ui, banner: &Banner) {
    let (fill, stroke) = match banner.tone {
        BannerTone::Success => {
            (Color32::from_rgba_unmultiplied(68, 255, 161, 30), OK)
        }
        BannerTone::Warn => {
            (Color32::from_rgba_unmultiplied(255, 198, 79, 30), WARN)
        }
        BannerTone::Error => {
            (Color32::from_rgba_unmultiplied(255, 107, 107, 35), ERR)
        }
    };
    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .rounding(egui::Rounding::same(12.0))
        .inner_margin(egui::Margin {
            left: 14.0,
            right: 14.0,
            top: 10.0,
            bottom: 10.0,
        })
        .show(ui, |ui| {
            ui.label(RichText::new(&banner.text).size(12.0).color(TEXT));
        });
}

// ── Global visual setup ───────────────────────────────────────────────────────

pub fn setup(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = WIN;
    visuals.panel_fill = WIN;
    visuals.extreme_bg_color = CHART_BG;
    visuals.faint_bg_color = CARD_ALT;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(68, 255, 161, 55);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_rounding = egui::Rounding::same(16.0);
    visuals.menu_rounding = egui::Rounding::same(12.0);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.widgets.noninteractive.bg_fill = CARD_ALT;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, SOFT);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 39, 46);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 49, 58);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
    visuals.widgets.active.rounding = egui::Rounding::same(8.0);
    visuals.widgets.open.bg_fill = Color32::from_rgb(34, 39, 46);
    visuals.widgets.open.rounding = egui::Rounding::same(8.0);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = vec2(8.0, 7.0);
    style.spacing.button_padding = vec2(14.0, 8.0);
    style.spacing.combo_width = 160.0;
    style.spacing.slider_width = 180.0;
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(14.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}

// ── Colour utils (used by tabs) ───────────────────────────────────────────────

pub fn colour_from_rgb(rgb: [f32; 3]) -> Color32 {
    Color32::from_rgb(
        (rgb[0].clamp(0.0, 1.0) * 255.0) as u8,
        (rgb[1].clamp(0.0, 1.0) * 255.0) as u8,
        (rgb[2].clamp(0.0, 1.0) * 255.0) as u8,
    )
}

pub fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
