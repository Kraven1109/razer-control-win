//! razer-gui — Razer Blade Control GUI for Windows.
//!
//! Module layout:
//!   main.rs      — entry point, eframe::App impl, sidebar nav
//!   constants.rs — color palette, effect tables, UI constants
//!   app.rs       — App struct, all shared data types, apply_poll, apply_effect
//!   poll.rs      — background poll thread, IPC send helper
//!   widgets.rs   — reusable widget helpers, global style setup
//!   tabs/        — per-tab draw functions

#![windows_subsystem = "windows"]

#[path = "../comms.rs"]
mod comms;

mod constants;
mod app;
mod poll;
mod widgets;
mod tabs;
mod display;
mod gui_config;
mod tray;

use app::{App, Tab};
use constants::{ACCENT, BORDER, DIM, ERR, OK, SIDEBAR, SOFT, TEXT, WIN};
use std::sync::Arc;

use eframe::egui::{
    self,
    vec2, Align2, Color32, CursorIcon, FontId, RichText, Sense, Stroke,
};

// ── App icon ──────────────────────────────────────────────────────────────────

/// Window icon rasterized from the bundled SVG at 64×64.
fn make_icon() -> Arc<egui::viewport::IconData> {
    const SVG: &[u8] = include_bytes!("../../data/razer-blade-control.svg");
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(SVG, &options)
        .expect("icon SVG parse failed");
    let size = 64u32;
    let sx = size as f32 / tree.size().width();
    let sy = size as f32 / tree.size().height();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
        .expect("pixmap alloc");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    Arc::new(egui::viewport::IconData {
        rgba: pixmap.data().to_vec(),
        width: size,
        height: size,
    })
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Tray menu actions ─────────────────────────────────────────────────
        if let Some(ref tray_rx) = self.tray_rx {
            while let Ok(action) = tray_rx.try_recv() {
                match action {
                    tray::TrayAction::ShowWindow => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    tray::TrayAction::SetProfile(mode) => {
                        let (cpu, gpu) = match mode {
                            0 => (0, 0), // Balanced
                            1 => (1, 1), // Gaming
                            2 => (2, 2), // Creator
                            _ => (3, 3), // Silent
                        };
                        let _ = crate::poll::send(comms::DaemonCommand::SetPowerMode {
                            ac: 1, pwr: mode, cpu, gpu,
                        });
                        let _ = crate::poll::send(comms::DaemonCommand::SetPowerMode {
                            ac: 0, pwr: mode, cpu, gpu,
                        });
                        self.wake_poll();
                    }
                    tray::TrayAction::SetBrightness(pct) => {
                        let val = ((pct as u32 * 255) / 100) as u8;
                        let _ = crate::poll::send(comms::DaemonCommand::SetBrightness { ac: 1, val });
                        let _ = crate::poll::send(comms::DaemonCommand::SetBrightness { ac: 0, val });
                        self.wake_poll();
                    }
                    tray::TrayAction::ToggleStartOnBoot => {
                        tray::toggle_autostart();
                    }
                    tray::TrayAction::Exit => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }

        // ── Poll results ──────────────────────────────────────────────────────
        // Drain all results from the background thread (non-blocking).
        while let Ok(data) = self.poll_rx.try_recv() {
            self.apply_poll(data);
        }
        self.trim_banner();

        // ── Detached chart window ─────────────────────────────────────────────
        if self.chart_detached {
            let chart_history = self.history.clone();
            let chart_gpu     = self.gpu.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("chart_float"),
                egui::ViewportBuilder::default()
                    .with_title("Razer — GPU monitor")
                    .with_inner_size([560.0, 360.0])
                    .with_always_on_top()
                    .with_resizable(true),
                |ctx, _class| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(constants::WIN))
                        .show(ctx, |ui| {
                            ui.add_space(8.0);
                            egui::Frame::none()
                                .inner_margin(egui::Margin::symmetric(16.0, 0.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("GPU Monitor")
                                                .size(16.0)
                                                .strong()
                                                .color(TEXT),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button(
                                                        RichText::new("Attach back")
                                                            .size(12.0)
                                                            .color(ACCENT),
                                                    )
                                                    .clicked()
                                                {
                                                    ctx.data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("chart_close"),
                                                            true,
                                                        );
                                                    });
                                                    ctx.send_viewport_cmd(
                                                        egui::ViewportCommand::Close,
                                                    );
                                                }
                                            },
                                        );
                                    });
                                    ui.add_space(8.0);
                                    tabs::system::draw_charts_only(
                                        ui,
                                        &chart_history,
                                        &chart_gpu,
                                    );
                                });
                        });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        ctx.data_mut(|d| d.insert_temp(egui::Id::new("chart_close"), true));
                    }
                },
            );

            // After viewport callback: check if attach-back was requested.
            if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("chart_close")).unwrap_or(false)) {
                self.chart_detached = false;
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("chart_close"), false));
            }
        }

        // ── Sidebar ───────────────────────────────────────────────────────────
        egui::SidePanel::left("nav")
            .exact_width(150.0)
            .frame(
                egui::Frame::none()
                    .fill(SIDEBAR)
                    .inner_margin(egui::Margin {
                        left: 12.0,
                        right: 12.0,
                        top: 20.0,
                        bottom: 12.0,
                    }),
            )
            .show(ctx, |ui| {
                // ── App header ────────────────────────────────────────────────
                ui.label(RichText::new("RAZER BLADE").size(9.5).color(ACCENT).strong());
                ui.add_space(2.0);
                ui.label(RichText::new("Control").size(18.0).color(TEXT).strong());
                ui.add_space(14.0);

                // Thin horizontal rule
                {
                    let w = ui.available_width();
                    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
                    ui.painter_at(r).rect_filled(r, 0.0, BORDER);
                }
                ui.add_space(12.0);

                // ── Nav items ─────────────────────────────────────────────────
                let btn_w = ui.available_width();
                const BTN_H: f32 = 44.0;

                for (tag, label, tab) in [
                    ("AC",  "Power",    Tab::Ac),
                    ("BAT", "Battery",  Tab::Battery),
                    ("KBD", "Keyboard", Tab::Keyboard),
                    ("SYS", "System",   Tab::System),
                ] {
                    let active  = self.tab == tab;
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(btn_w, BTN_H), Sense::click());
                    let hovered = resp.hovered();
                    let pressed = resp.is_pointer_button_down_on();

                    let fill = if active {
                        Color32::from_rgba_unmultiplied(68, 255, 161, 22)
                    } else if pressed {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 10)
                    } else if hovered {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 5)
                    } else {
                        Color32::TRANSPARENT
                    };

                    let p = ui.painter_at(rect);
                    // Rounded background — no stroke border.
                    p.rect_filled(rect, egui::Rounding::same(10.0), fill);

                    // Active-state left accent bar.
                    if active {
                        let bar = egui::Rect::from_min_size(
                            rect.left_top(),
                            vec2(3.0, BTN_H),
                        );
                        p.rect_filled(bar, egui::Rounding::same(1.5), ACCENT);
                    }

                    // Badge — subtle filled square, no border.
                    let badge_col = if active { ACCENT } else if hovered { TEXT } else { SOFT };
                    let badge_fill = Color32::from_rgba_unmultiplied(
                        badge_col.r(), badge_col.g(), badge_col.b(),
                        if active { 35 } else { 16 },
                    );
                    let b_size = vec2(28.0, 22.0);
                    let b_left = 10.0_f32;
                    let b_pos  = rect.left_top() + vec2(b_left, (BTN_H - b_size.y) / 2.0);
                    let badge  = egui::Rect::from_min_size(b_pos, b_size);
                    p.rect_filled(badge, egui::Rounding::same(6.0), badge_fill);
                    p.text(
                        badge.center(),
                        Align2::CENTER_CENTER,
                        tag,
                        FontId::proportional(9.0),
                        badge_col,
                    );

                    // Label.
                    let text_c = if active || hovered { TEXT } else { DIM };
                    p.text(
                        rect.left_top() + vec2(48.0, BTN_H / 2.0),
                        Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(12.5),
                        text_c,
                    );

                    if hovered { ctx.set_cursor_icon(CursorIcon::PointingHand); }
                    if resp.clicked() { self.tab = tab; }
                    ui.add_space(4.0);
                }

                // ── Bottom status ─────────────────────────────────────────────
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(4.0);

                    // Daemon dot + status text.
                    let (st, sc) = if self.ok { ("ONLINE", OK) } else { ("OFFLINE", ERR) };
                    ui.horizontal(|ui| {
                        let (dot_r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                        ui.painter_at(dot_r).circle_filled(dot_r.center(), 3.5, sc);
                        ui.label(RichText::new(st).size(10.5).color(sc).strong());
                    });
                    ui.add_space(3.0);

                    // GPU source label.
                    let gpu_src = self
                        .gpu
                        .as_ref()
                        .map(|g| g.name.as_str())
                        .unwrap_or("unavailable");
                    let gpu_short = if gpu_src.contains("Task Manager") || gpu_src.contains("PDH") {
                        "GPU: PDH"
                    } else if gpu_src.contains("unavailable") {
                        "GPU: N/A"
                    } else {
                        "GPU: live"
                    };
                    ui.label(RichText::new(gpu_short).size(10.0).color(SOFT));
                });
            });

        // ── Central panel ─────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(WIN))
            .show(ctx, |ui| {
                if !self.ok {
                    ui.centered_and_justified(|ui| {
                        egui::Frame::none()
                            .fill(constants::PANEL)
                            .stroke(Stroke::new(1.0, BORDER))
                            .rounding(egui::Rounding::same(18.0))
                            .inner_margin(egui::Margin { left: 26.0, right: 26.0, top: 24.0, bottom: 24.0 })
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Cannot connect to razer-daemon")
                                        .size(18.0).strong().color(TEXT),
                                );
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(
                                        "Run razer-daemon.exe as Administrator, then reopen the GUI.",
                                    )
                                    .size(12.0)
                                    .color(DIM),
                                );
                            });
                    });
                    return;
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        let pad = ((width - 760.0) / 2.0).max(18.0);
                        egui::Frame::none()
                            .inner_margin(egui::Margin {
                                left: pad,
                                right: pad,
                                top: 14.0,
                                bottom: 26.0,
                            })
                            .show(ui, |ui| {
                                if let Some(ref banner) = self.banner {
                                    widgets::draw_banner(ui, banner);
                                    ui.add_space(8.0);
                                }
                                // Always run temperature-target controller (both slots) so the
                                // fan responds even when the user is on a different tab.
                                for is_ac in [true, false] {
                                    if let Some(new_rpm) = self.apply_temp_control(is_ac) {
                                        let _ = crate::poll::send(comms::DaemonCommand::SetFanSpeed {
                                            ac: if is_ac { 1 } else { 0 },
                                            rpm: new_rpm,
                                        });
                                    }
                                }
                                match self.tab {
                                    Tab::Ac       => tabs::power::draw_power(self, ui, true),
                                    Tab::Battery  => tabs::power::draw_power(self, ui, false),
                                    Tab::Keyboard => tabs::keyboard::draw_keyboard(self, ui),
                                    Tab::System   => tabs::system::draw_system(self, ui),
                                }
                            });
                    });
            });
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Razer Blade Control")
            .with_inner_size([980.0, 760.0])
            .with_min_inner_size([820.0, 620.0])
            .with_icon(make_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Razer Blade Control",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}
