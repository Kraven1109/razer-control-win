/// Keyboard lighting effects tab.

use crate::app::{App, BannerTone};
use crate::comms;
use crate::constants::*;
use crate::poll::send;
use crate::widgets::{card, colour_from_rgb, draw_page_header, lerp_rgb, row, rowsep};
use eframe::egui::{
    self, color_picker, pos2, vec2, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui,
};

// ── Keyboard preview ──────────────────────────────────────────────────────────

pub fn draw_keyboard_preview(ui: &mut Ui, app: &App) {
    let eidx = app.eidx;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 90.0), Sense::hover());
    let p = ui.painter();

    let body_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 14);
    p.rect_filled(rect, egui::Rounding::same(10.0), body_fill);
    p.rect_stroke(
        rect,
        egui::Rounding::same(10.0),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 32)),
    );

    // Row geometry.
    let row_heights = [12.0_f32, 11.0, 11.0, 11.0];
    let col_counts = [14_usize, 14, 13, 11];
    let row_gap = 3.0;
    let col_gap = 2.0;
    let pad_x = 12.0;
    let pad_y = 11.0;
    let total_h: f32 = row_heights.iter().sum::<f32>() + row_gap * 3.0;
    let key_w = (rect.width() - 2.0 * pad_x - col_gap * 13.0) / 14.0;
    let start_y = rect.min.y + pad_y + (rect.height() - total_h - 2.0 * pad_y) / 2.0;

    let mut keys: Vec<(Pos2, [f32; 2])> = Vec::with_capacity(52);
    let mut y = start_y;
    for (row_i, (h, cols)) in row_heights.iter().zip(col_counts.iter()).enumerate() {
        let row_w = key_w * (*cols as f32) + col_gap * (cols - 1) as f32;
        let indent = if row_i > 0 {
            (rect.width() - 2.0 * pad_x - row_w) * 0.0
        } else {
            0.0
        };
        let x0 = rect.min.x + pad_x + indent;
        for col in 0..*cols {
            let kx = x0 + col as f32 * (key_w + col_gap);
            keys.push((pos2(kx, y), [key_w, *h]));
        }
        y += h + row_gap;
    }

    // Colour each key based on the active effect.
    let num_keys = keys.len() as f32;
    for (i, (pos, size)) in keys.iter().enumerate() {
        let t = i as f32 / (num_keys - 1.0).max(1.0);
        let col: Color32 = match eidx {
            0 => colour_from_rgb(app.c1),
            1 => {
                let a = if t < 0.5 {
                    lerp_rgb(app.c1, app.c2, t * 2.0)
                } else {
                    lerp_rgb(app.c2, app.c1, (t - 0.5) * 2.0)
                };
                colour_from_rgb(a)
            }
            2 => {
                let a = lerp_rgb(app.c1, app.c2, t);
                colour_from_rgb(a)
            }
            3 | 4 => {
                let hue = (t + 0.3) % 1.0;
                rainbow_hue(hue)
            }
            5 => rainbow_hue(t),
            // Rainbow wave / Wheel — dir=1 means left→right (lower index = earlier wave front)
            6 | 9 => {
                let t_dir = if app.dir == 1 { t } else { 1.0 - t };
                rainbow_hue(t_dir)
            }
            7 => {
                let a = lerp_rgb(
                    app.c1,
                    [app.c1[0] * 0.3, app.c1[1] * 0.3, app.c1[2] * 0.3],
                    (t * 3.0).fract(),
                );
                colour_from_rgb(a)
            }
            8 => {
                let a = lerp_rgb(
                    app.c1,
                    [app.c1[0] * 0.15, app.c1[1] * 0.15, app.c1[2] * 0.15],
                    ((t * 2.0).fract() - 0.5).abs() * 2.0,
                );
                colour_from_rgb(a)
            }
            // All other indices: fall back to primary colour
            _ => colour_from_rgb(app.c1),
        };

        let key_rect = Rect::from_min_size(*pos, vec2(size[0], size[1]));
        p.rect_filled(key_rect, egui::Rounding::same(3.0), col);
    }
}

fn rainbow_hue(t: f32) -> Color32 {
    let h = t.fract();
    // Convert via egui's built-in HsvaGamma → Color32 path.
    Color32::from(egui::ecolor::HsvaGamma { h, s: 1.0, v: 0.9, a: 1.0 })
}

// ── Keyboard lighting tab ─────────────────────────────────────────────────────

pub fn draw_keyboard(app: &mut App, ui: &mut Ui) {
    draw_page_header(
        ui,
        "Keyboard Studio",
        "Live preview — effect is not applied until you press Apply.",
        app,
    );

    card(ui, "Lighting Effect", effect_desc(app.eidx), |ui| {
        let old_eidx = app.eidx;
        row(ui, "Effect", "Custom animated keyboard effect", |ui| {
            egui::ComboBox::from_id_salt("kbd_effect")
                .selected_text(effect_label(app.eidx))
                .width(210.0)
                .show_ui(ui, |ui| {
                    for (idx, label) in EFFECT_LABELS.iter().enumerate() {
                        ui.selectable_value(&mut app.eidx, idx, *label);
                    }
                });
        });
        if app.eidx != old_eidx {
            app.effect_dirty = true;
        }

        let flags = EFFECT_FLAGS[app.eidx.min(EFFECT_FLAGS.len() - 1)];
        if flags.c1 || flags.c2 || flags.spd || flags.dir || flags.den || flags.dur {
            rowsep(ui);
        }

        if flags.c1 {
            row(ui, "Primary colour", "", |ui| {
                if color_picker::color_edit_button_rgb(ui, &mut app.c1).changed() {
                    app.effect_dirty = true;
                }
            });
        }
        if flags.c2 {
            if flags.c1 { rowsep(ui); }
            row(ui, "Secondary colour", "", |ui| {
                if color_picker::color_edit_button_rgb(ui, &mut app.c2).changed() {
                    app.effect_dirty = true;
                }
            });
        }
        if flags.spd {
            rowsep(ui);
            row(ui, "Speed", "1 - 10", |ui| {
                if ui.add(egui::Slider::new(&mut app.spd, 1_u8..=10_u8)).changed() {
                    app.effect_dirty = true;
                }
            });
        }
        if flags.dir {
            rowsep(ui);
            let old_dir = app.dir;
            row(ui, "Direction", "Animation flow", |ui| {
                // FIXED: dir=1 → left-to-right on device, dir=0 → right-to-left.
                // The original code had these labels inverted.
                egui::ComboBox::from_id_salt("kbd_dir")
                    .selected_text(if app.dir == 1 { "Left to right" } else { "Right to left" })
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.dir, 1, "Left to right");
                        ui.selectable_value(&mut app.dir, 0, "Right to left");
                    });
            });
            if app.dir != old_dir {
                app.effect_dirty = true;
            }
        }
        if flags.den {
            rowsep(ui);
            row(ui, "Density", "1 - 20", |ui| {
                if ui.add(egui::Slider::new(&mut app.den, 1_u8..=20_u8)).changed() {
                    app.effect_dirty = true;
                }
            });
        }
        if flags.dur {
            rowsep(ui);
            row(ui, "Duration", "Cycle × 100 ms", |ui| {
                if ui.add(egui::Slider::new(&mut app.dur, 1_u8..=20_u8)).changed() {
                    app.effect_dirty = true;
                }
            });
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let button =
                egui::Button::new(RichText::new("Apply effect").color(Color32::BLACK).strong())
                    .fill(ACCENT)
                    .rounding(egui::Rounding::same(10.0))
                    .min_size(vec2(150.0, 34.0));
            if ui.add_enabled(app.ok, button).clicked() {
                app.apply_effect();
            }
            ui.label(RichText::new(effect_key(app.eidx)).size(11.0).color(DIM));
        });
    });

    card(ui, "Live Preview", "Preview only — the daemon applies the final effect", |ui| {
        draw_keyboard_preview(ui, app);
    });

    // ── Battery health optimizer ─────────────────────────────────────────────
    let old_bho = (app.bho, app.bho_thr);
    card(
        ui,
        "Battery Health Optimizer",
        "Limit charge threshold to extend battery lifespan",
        |ui| {
            row(ui, "Enabled", "Charge stops at threshold", |ui| {
                ui.checkbox(&mut app.bho, "");
            });
            if app.bho {
                rowsep(ui);
                row(ui, "Threshold", "50 - 80 %", |ui| {
                    ui.add(
                        egui::Slider::new(&mut app.bho_thr, 50_u8..=80_u8)
                            .suffix("%")
                            .clamping(egui::SliderClamping::Always),
                    );
                });
            }
        },
    );
    if (app.bho, app.bho_thr) != old_bho {
        if !matches!(
            send(comms::DaemonCommand::SetBatteryHealthOptimizer {
                is_on: app.bho,
                threshold: app.bho_thr,
            }),
            Some(comms::DaemonResponse::SetBatteryHealthOptimizer { result: true })
        ) {
            app.set_banner(
                BannerTone::Warn,
                "Battery optimizer change was not acknowledged by the daemon.",
            );
        } else {
            app.wake_poll();
        }
    }
}
