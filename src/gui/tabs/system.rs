/// System monitor tab — GPU metrics card and history charts.

use crate::app::{App, GpuInfo, Sample};
use crate::constants::*;
use crate::widgets::{card, card_inner, chip, draw_page_header, metric_tile, row as info_row};
use eframe::egui::{
    self, pos2, vec2, Color32, Pos2, RichText, Sense, Stroke, Ui,
};
use std::collections::VecDeque;

// ── Timeline chart ────────────────────────────────────────────────────────────

/// Unified multi-line timeline chart — all 4 metrics on one canvas.
/// Mirrors the Linux Cairo-based chart style: left Y = °C/%, right Y = Watts,
/// grid lines, TGP dashed reference line, legend.
pub fn draw_timeline_chart(
    ui: &mut Ui,
    _id: &str,
    history: &VecDeque<Sample>,
    tgp_limit_w: f64,
    height: f32,
) {
    let n = history.len();
    let desired_h = height;
    let desired_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        vec2(desired_w, desired_h),
        Sense::hover(),
    );
    let p = ui.painter();

    // Background.
    p.rect_filled(rect, egui::Rounding::same(10.0), CHART_BG);
    p.rect_stroke(rect, egui::Rounding::same(10.0), Stroke::new(1.0, BORDER));

    if n < 2 {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for data…",
            egui::FontId::proportional(11.5),
            Color32::from_rgba_unmultiplied(255, 255, 255, 80),
        );
        return;
    }

    // Layout padding (mirrors Linux: padl for left axis, padr for right axis labels).
    let pad_l = 24.0_f32;
    let pad_r = 54.0_f32;
    let pad_t = 10.0_f32;
    let pad_b = 24.0_f32;
    let cw = rect.width() - pad_l - pad_r;
    let ch = rect.height() - pad_t - pad_b;
    let plot_x0 = rect.min.x + pad_l;
    let plot_y0 = rect.min.y + pad_t;

    // Grid lines (5 horizontal = 0%, 25%, 50%, 75%, 100%).
    for i in 0..=4 {
        let frac = i as f32 / 4.0;
        let gy = plot_y0 + ch * frac;
        p.line_segment(
            [pos2(plot_x0, gy), pos2(plot_x0 + cw, gy)],
            Stroke::new(if i == 0 || i == 4 { 1.0 } else { 0.5 }, BORDER),
        );
    }

    // Y-axis labels: left = %/°C  (orange for temp scale), right = GPU%+VRAM%, rightmost = W.
    let fs = 9.5_f32;
    for i in 0..=4 {
        let val = (4 - i) * 25;   // 100, 75, 50, 25, 0
        let watts = val * 2;       // 200, 150, 100, 50, 0
        let gy = plot_y0 + ch * (i as f32 / 4.0) + fs * 0.36;

        // Left: temp/% axis (orange tint).
        p.text(
            pos2(rect.min.x + 2.0, gy),
            egui::Align2::LEFT_CENTER,
            format!("{val}°"),
            egui::FontId::proportional(fs),
            Color32::from_rgba_unmultiplied(255, 150, 80, 200),
        );
        // Right: Watts only (right-aligned, stays within chart rect).
        p.text(
            pos2(rect.max.x - 4.0, gy),
            egui::Align2::RIGHT_CENTER,
            format!("{watts}W"),
            egui::FontId::proportional(fs),
            Color32::from_rgba_unmultiplied(80, 200, 255, 180),
        );
    }

    let x_step = cw / (n - 1).max(1) as f32;

    // Helper: draw a polyline for one metric.  Accepts an iterator directly so
    // no intermediate Vec<f64> is allocated (4 allocations eliminated per frame).
    let draw_line = |painter: &egui::Painter, val_iter: &mut dyn Iterator<Item = f64>, color: Color32| {
        let pts: Vec<Pos2> = val_iter
            .enumerate()
            .map(|(i, v)| {
                let x = plot_x0 + i as f32 * x_step;
                // All series are already normalised to 0-100 %.
                let y = plot_y0 + ch * (1.0 - (v / 100.0).clamp(0.0, 1.0) as f32);
                pos2(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.8, color)));
    };

    let has_temp  = history.iter().any(|s| s.temp_c  > 0.0);
    let has_power = history.iter().any(|s| s.power_w > 0.0);

    // Draw lines (order = back → front) — stream directly from history.
    if has_power {
        draw_line(p, &mut history.iter().map(|s| s.power_w * 0.5), CH_POWER);
    }
    if has_temp {
        draw_line(p, &mut history.iter().map(|s| s.temp_c), CH_TEMP);
    }
    draw_line(p, &mut history.iter().map(|s| s.vram_pct), CH_VRAM);
    draw_line(p, &mut history.iter().map(|s| s.gpu_pct),  CH_GPU);

    // TGP limit — dashed reference (same as Linux version).
    if tgp_limit_w > 0.0 {
        let tgp_frac = (tgp_limit_w / 200.0).clamp(0.0, 1.0) as f32;
        let tgp_y = plot_y0 + ch * (1.0 - tgp_frac);
        let dash_col = Color32::from_rgba_unmultiplied(CH_POWER.r(), CH_POWER.g(), CH_POWER.b(), 90);
        let dash_len = 6.0_f32;
        let gap_len  = 5.0_f32;
        let mut x = plot_x0;
        while x < plot_x0 + cw {
            let x_end = (x + dash_len).min(plot_x0 + cw);
            p.line_segment([pos2(x, tgp_y), pos2(x_end, tgp_y)], Stroke::new(1.5, dash_col));
            x += dash_len + gap_len;
        }
        // TGP label — clamp above top padding so it never bleeds outside.
        let tgp_label_y = (tgp_y - 9.0).max(rect.min.y + pad_t + 1.0);
        p.text(
            pos2(plot_x0 + 3.0, tgp_label_y),
            egui::Align2::LEFT_CENTER,
            format!("TGP {:.0}W", tgp_limit_w),
            egui::FontId::proportional(fs * 0.85),
            Color32::from_rgba_unmultiplied(CH_POWER.r(), CH_POWER.g(), CH_POWER.b(), 160),
        );
    }

    // Legend — 4 items evenly spaced at the bottom.
    let legend_items: &[(&str, Color32)] = &[
        ("Temp",  CH_TEMP),
        ("GPU%",  CH_GPU),
        ("VRAM%", CH_VRAM),
        ("Power", CH_POWER),
    ];
    let n_leg = legend_items.len() as f32;
    let leg_step = cw / n_leg;
    let leg_y = rect.max.y - 10.0;
    let box_sz = 7.0_f32;
    for (idx, (label, color)) in legend_items.iter().enumerate() {
        let lx = plot_x0 + idx as f32 * leg_step + (leg_step / 2.0 - 22.0);
        p.rect_filled(
            egui::Rect::from_min_size(pos2(lx, leg_y - box_sz + 1.0), vec2(box_sz, box_sz)),
            egui::Rounding::same(1.0),
            *color,
        );
        p.text(
            pos2(lx + box_sz + 3.0, leg_y),
            egui::Align2::LEFT_BOTTOM,
            *label,
            egui::FontId::proportional(fs),
            *color,
        );
    }
}


fn draw_tile_row(
    ui: &mut Ui,
    tile_count: usize,
    tile_h: f32,
    gap: f32,
    mut draw_tile: impl FnMut(&mut Ui, usize),
) {
    if tile_count == 0 {
        return;
    }

    let tile_w = if tile_count == 1 {
        ui.available_width()
    } else {
        (ui.available_width() - gap * (tile_count.saturating_sub(1) as f32)) / tile_count as f32
    };

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for tile_idx in 0..tile_count {
            ui.allocate_ui_with_layout(
                vec2(tile_w, tile_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(tile_w);
                    ui.set_max_width(tile_w);
                    draw_tile(ui, tile_idx);
                },
            );
        }
    });
}

// ── System tab ────────────────────────────────────────────────────────────────

pub fn draw_system(app: &mut App, ui: &mut Ui) {
    draw_page_header(
        ui,
        "System Monitor",
        "Real-time GPU metrics and history charts.",
        app,
    );

    // ── GPU metrics row ────────────────────────────────────────────────────────
    match &app.gpu {
        None => {
            card(ui, "GPU", "Connect to daemon to receive GPU info.", |ui| {
                ui.label(
                    RichText::new("No data — daemon offline or GPU unreachable.")
                        .color(SOFT)
                        .size(13.0),
                );
            });
        }
        Some(gpu) => {
            // Snapshot app-level values before gpu borrow deepens.
            let cpu_pct      = app.sys.cpu_pct;
            let ram_used_mb  = app.sys.ram_used_mb;
            let ram_total_mb = app.sys.ram_total_mb;
            let fan_rpm      = app.ac.fan;       // Razer EC fan RPM

            // Capture total_w once — everything in this arm (side-by-side cards,
            // History chart, System Info) is pinned to this exact width so they
            // all align with each other and respect the surrounding padding.
            let total_w = ui.available_width();

            // ── Side-by-side: GPU card (left 50%) + System card (right 50%) ──
            {
                const TILE_H: f32 = 96.0;
                const SPC: f32 = 4.0;
                const CARD_GAP: f32 = 14.0;
                // let left_w = 210.0;
                let left_w = ((total_w - CARD_GAP) * 0.57).floor();
                let right_w = (total_w - CARD_GAP - left_w).max(0.0);

                // Pre-compute all tile data before the layout borrowing begins.
                let has_temp  = gpu.temp > 0;
                let has_power = gpu.power_w > 0.0;
                let has_clk   = gpu.clk_gpu_mhz > 0;

                let tc = if gpu.temp < 70 { OK } else if gpu.temp < 85 { WARN } else { ERR };
                let vram_pct = if gpu.mem_total_mb > 0 {
                    gpu.mem_used_mb * 100 / gpu.mem_total_mb.max(1)
                } else { gpu.mem_util as u32 };
                let vram_label  = format!("{} MB", gpu.mem_used_mb);
                let vram_sub = if gpu.mem_total_mb > 0 {
                    let p = if vram_pct == 0 { "< 1%".into() } else { format!("{}%", vram_pct) };
                    format!("/ {} MB  ·  {}", gpu.mem_total_mb, p)
                } else { "VRAM in use".into() };
                let tgp_str    = format!("{:.0} W", gpu.power_w);
                let clk_str    = format!("{} MHz", gpu.clk_gpu_mhz);
                let gpu_util   = gpu.util;
                let gpu_temp   = gpu.temp;
                let power_lim  = gpu.power_limit_w;
                let clk_mem    = gpu.clk_mem_mhz;
                let gpu_stale  = gpu.stale;
                let gpu_name   = gpu.name.clone();

                ui.add_space(10.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;

                    // ── Left: GPU metrics card ──────────────────────────────
                    ui.allocate_ui_with_layout(vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_min_width(left_w);
                        ui.set_max_width(left_w);
                        card_inner(ui, "GPU", &gpu_name, |ui| {
                            draw_tile_row(ui, 3, TILE_H, SPC, |ui, tile_idx| {
                                match tile_idx {
                                    0 => {
                                        metric_tile(ui, "GPU", format!("{}%", gpu_util), "Utilization", CH_GPU);
                                    }
                                    1 => {
                                        if has_temp {
                                            // Accent matches the chart line; a faint background tint
                                            // still signals hot/warm/ok via the tile border shade.
                                            let _ = tc; // kept for future border tint use
                                            metric_tile(ui, "Temp", format!("{} °C", gpu_temp), "Sensor", CH_TEMP);
                                        } else {
                                            metric_tile(ui, "Temp", "--", "No sensor", SOFT);
                                        }
                                    }
                                    _ => {
                                        metric_tile(ui, "VRAM", vram_label.clone(), vram_sub.clone(), CH_VRAM);
                                    }
                                }
                            });
                            ui.add_space(SPC);
                            draw_tile_row(ui, 2, TILE_H, SPC, |ui, tile_idx| {
                                match tile_idx {
                                    0 => {
                                        if has_power {
                                            metric_tile(
                                                ui,
                                                "TGP",
                                                tgp_str.clone(),
                                                if power_lim > 0.0 {
                                                    format!("/ {:.0} W limit", power_lim)
                                                } else {
                                                    "Power draw".into()
                                                },
                                                CH_POWER,
                                            );
                                        } else {
                                            metric_tile(ui, "TGP", "--", "No telemetry", SOFT);
                                        }
                                    }
                                    _ => {
                                        if has_clk {
                                            metric_tile(
                                                ui,
                                                "Core Clock",
                                                clk_str.clone(),
                                                if clk_mem > 0 {
                                                    format!("{} MHz mem", clk_mem)
                                                } else {
                                                    "GPU core".into()
                                                },
                                                DIM,
                                            );
                                        } else {
                                            metric_tile(ui, "Core Clock", "--", "No telemetry", SOFT);
                                        }
                                    }
                                }
                            });
                            if gpu_stale {
                                ui.add_space(SPC);
                                ui.horizontal(|ui| {
                                    chip(ui, "Stale data — counters may be delayed",
                                        Color32::from_rgba_unmultiplied(255, 198, 79, 30), WARN, WARN);
                                });
                            }
                        });
                    });

                    ui.add_space(CARD_GAP);

                    ui.allocate_ui_with_layout(vec2(right_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_min_width(right_w);
                        ui.set_max_width(right_w);
                        card_inner(ui, "System", "Performance counters", |ui| {
                            let has_fan  = fan_rpm > 0;
                            let cpu_col  = if cpu_pct < 60.0 { OK } else if cpu_pct < 85.0 { WARN } else { ERR };
                            let ram_pct  = if ram_total_mb > 0 { ram_used_mb * 100 / ram_total_mb } else { 0 };
                            let ram_col  = if ram_pct < 70 { OK } else if ram_pct < 85 { WARN } else { ERR };
                            let fmt_mb   = |mb: u64| -> String {
                                if mb >= 1024 { format!("{:.1} GB", mb as f64 / 1024.0) }
                                else          { format!("{} MB", mb) }
                            };
                            let ram_sub = format!("/ {}  ·  {}% used", fmt_mb(ram_total_mb), ram_pct);
                            draw_tile_row(ui, 2, TILE_H, SPC, |ui, tile_idx| {
                                match tile_idx {
                                    0 => metric_tile(ui, "CPU", format!("{:.0}%", cpu_pct), "Utilization", cpu_col),
                                    _ => metric_tile(ui, "RAM", fmt_mb(ram_used_mb), ram_sub.clone(), ram_col),
                                }
                            });
                            // ── Thermal + Fan row (CPU temp always shown; Fan only when available) ──
                            let cpu_temp_c = app.sys.cpu_temp_c;
                            let cpu_temp_str = if cpu_temp_c > 0.0 { format!("{:.0} °C", cpu_temp_c) } else { "--".to_string() };
                            let tile_count = 1 + has_fan as usize;
                            ui.add_space(SPC);
                            draw_tile_row(ui, tile_count, TILE_H, SPC, |ui, tile_idx| {
                                match tile_idx {
                                    0 => metric_tile(ui, "CPU Temp", cpu_temp_str.clone(), "ACPI thermal zone", CH_TEMP),
                                    _ => metric_tile(ui, "Fan", format!("{} RPM", fan_rpm), "Razer EC", DIM),
                                }
                            });
                        });
                    });
                });
            }

            // History card — pinned to total_w so it matches the side-by-side cards.
            ui.allocate_ui_with_layout(vec2(total_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                if app.chart_detached {
                    card(ui, "History", "Chart is open in the floating monitor window", |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("GPU monitor is detached.").size(12.0).color(SOFT));
                            if ui.button(RichText::new("Attach back").size(12.0).color(ACCENT)).clicked() {
                                app.chart_detached = false;
                            }
                        });
                    });
                } else {
                    card(ui, "History", "Last 20 samples (~60 s)", |ui| {
                        draw_chart_body(ui, &app.history, &app.gpu);
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if ui
                                .button(RichText::new("⤢ Detach").size(11.5).color(ACCENT))
                                .on_hover_text("Open chart in a floating always-on-top window")
                                .clicked()
                            {
                                app.chart_detached = true;
                            }
                        });
                    });
                }
            });

            // ── System info card — also pinned to total_w ──────────────────────
            ui.allocate_ui_with_layout(vec2(total_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                draw_system_info(ui, app);
            });
        }
    }

    // ── Startup settings card — always visible regardless of GPU availability ─
    draw_startup_card(ui);
}

fn draw_startup_card(ui: &mut Ui) {
    card(ui, "Startup", "System integration settings", |ui| {
        ui.set_min_width(ui.available_width());

        let autostart = crate::tray::is_autostart_enabled();
        info_row(ui, "Start on boot", "Launch GUI minimized to tray on Windows login", |ui| {
            let mut checked = autostart;
            if ui.checkbox(&mut checked, "").changed() {
                crate::tray::toggle_autostart();
            }
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                "Registers razer-gui.exe in the current user's Run registry key. \
                 The daemon must be started separately with Administrator privileges \
                 (e.g. via a Task Scheduler entry set to run elevated at logon).",
            )
            .size(11.0)
            .color(SOFT),
        );
    });
}

fn draw_system_info(ui: &mut Ui, app: &App) {
    let si = &app.sys_static;
    // Don't render if nothing is collected yet.
    if si.cpu_name.is_empty() && si.os_name.is_empty() && si.laptop_model.is_empty() {
        return;
    }

    // Uptime: d h m
    let uptime_str = {
        let s = si.uptime_secs;
        let d = s / 86400;
        let h = (s % 86400) / 3600;
        let m = (s % 3600) / 60;
        if d > 0 { format!("{}d {}h {}m", d, h, m) }
        else if h > 0 { format!("{}h {}m", h, m) }
        else { format!("{}m", m) }
    };

    // BIOS: combine version + date (if both present).
    let bios_str = match (si.bios_version.is_empty(), si.bios_date.is_empty()) {
        (false, false) => format!("{}  ({})", si.bios_version, si.bios_date),
        (false, true)  => si.bios_version.clone(),
        (true,  false) => si.bios_date.clone(),
        (true,  true)  => String::new(),
    };

    // Build rows: only show non-empty fields, skip fields already in tiles above.
    let rows: &[(&str, &str)] = &[
        ("Model",   &si.laptop_model),
        ("BIOS",    &bios_str),
        ("CPU",     &si.cpu_name),
        ("OS",      &si.os_name),
        ("Host",    &si.host_name),
        ("Uptime",  &uptime_str),
    ];

    card(ui, "System Info", "Hardware configuration", |ui| {
        // Force the card to span the full available width even though the
        // row labels are narrow (egui Frame shrinks to content by default).
        ui.set_min_width(ui.available_width());
        let mut any = false;
        for (label, value) in rows {
            if value.is_empty() { continue; }
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new(*label).size(12.0).color(DIM).strong());
                ui.add_space(8.0);
                ui.label(RichText::new(*value).size(12.0).color(TEXT));
            });
            ui.add_space(5.0);
            any = true;
        }
        if !any {
            ui.label(RichText::new("Collecting…").size(12.0).color(SOFT));
        }
    });
}

// ── Shared chart body helpers ─────────────────────────────────────────────────

/// Draw the chart grid (GPU%, VRAM%, Temp, Power) from any history slice.
fn draw_chart_body(ui: &mut Ui, history: &VecDeque<Sample>, gpu: &Option<GpuInfo>) {
    let tgp_limit = gpu.as_ref().map(|g| g.power_limit_w as f64).unwrap_or(0.0);
    draw_timeline_chart(ui, "unified", history, tgp_limit, 220.0);
}

/// Exposed variant for the detached window — fills available height.
pub fn draw_charts_only(ui: &mut Ui, history: &VecDeque<Sample>, gpu: &Option<GpuInfo>) {
    let h = (ui.available_height() - 2.0).max(180.0);
    let tgp_limit = gpu.as_ref().map(|g| g.power_limit_w as f64).unwrap_or(0.0);
    draw_timeline_chart(ui, "unified", history, tgp_limit, h);
}
