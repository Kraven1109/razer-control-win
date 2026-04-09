/// AC and Battery power profile tab.
///
/// IPC is sent ONLY when a widget fires .changed() or .drag_released().
/// This eliminates the render-loop spam that occurred with the old
/// before/after snapshot comparison approach.

use crate::app::{App, BannerTone};
use crate::comms;
use crate::poll::send;
use crate::widgets::{card, draw_page_header, row, rowsep};
use eframe::egui::{self, Id, Ui};

pub fn draw_power(app: &mut App, ui: &mut Ui, is_ac: bool) {
    let title    = if is_ac { "AC Profile"      } else { "Battery Profile"   };
    let subtitle = if is_ac {
        "Performance settings applied while the charger is connected."
    } else {
        "Performance settings applied when running on battery."
    };
    draw_page_header(ui, title, subtitle, app);

    let ac = if is_ac { 1usize } else { 0usize };
    let mut issues: Vec<&str> = Vec::new();
    let mut any_ok = false;
    let mut profile_write_ok = false;

    // Power Profile card
    card(ui, "Power Profile", "Mode, CPU boost and GPU boost", |ui| {
        let p = if is_ac { &mut app.ac } else { &mut app.bat };

        let mode_desc = match p.mode {
            0 => "Balanced — even split between performance and thermals",
            1 => "Gaming — max CPU + GPU boost, higher fan speed",
            2 => "Creator — sustained boost for creative workloads",
            3 => "Silent — reduced clocks, quiet fan profile",
            4 => "Custom — manual CPU / GPU boost levels below",
            _ => "",
        };

        row(ui, "Profile", mode_desc, |ui| {
            let old_mode = p.mode;
            egui::ComboBox::from_id_salt(Id::new("pwr_mode").with(ac))
                .selected_text(
                    ["Balanced", "Gaming", "Creator", "Silent", "Custom"]
                        [p.mode.min(4) as usize],
                )
                .width(156.0)
                .show_ui(ui, |ui| {
                    for (idx, label) in
                        ["Balanced", "Gaming", "Creator", "Silent", "Custom"]
                            .iter()
                            .enumerate()
                    {
                        ui.selectable_value(&mut p.mode, idx as u8, *label);
                    }
                });

            // Detect selection changes from the bound value, not the combo button response.
            if p.mode != old_mode {
                // For non-custom modes the daemon ignores cpu/gpu, but send 0
                // explicitly so it never inherits stale Custom-mode values.
                let (cpu_arg, gpu_arg) = if p.mode == 4 { (p.cpu, p.gpu) } else { (0, 0) };
                match send(comms::DaemonCommand::SetPowerMode {
                    ac,
                    pwr: p.mode,
                    cpu: cpu_arg,
                    gpu: gpu_arg,
                }) {
                    Some(comms::DaemonResponse::SetPowerMode { result: true }) => {
                        any_ok = true;
                        profile_write_ok = true;
                    }
                    _ => issues.push("power mode"),
                }
            }
        });

        // Custom-mode CPU / GPU boost options.
        if p.mode == 4 {
            rowsep(ui);
            row(ui, "CPU boost", "Processor performance level", |ui| {
                let old_cpu = p.cpu;
                egui::ComboBox::from_id_salt(Id::new("cpu_boost").with(ac))
                    .selected_text(
                        ["Low", "Medium", "High", "Boost"][p.cpu.min(3) as usize],
                    )
                    .width(156.0)
                    .show_ui(ui, |ui| {
                        for (idx, label) in
                            ["Low", "Medium", "High", "Boost"].iter().enumerate()
                        {
                            ui.selectable_value(&mut p.cpu, idx as u8, *label);
                        }
                    });
                if p.cpu != old_cpu {
                    match send(comms::DaemonCommand::SetPowerMode {
                        ac, pwr: 4, cpu: p.cpu, gpu: p.gpu,
                    }) {
                        Some(comms::DaemonResponse::SetPowerMode { result: true }) => {
                            any_ok = true;
                            profile_write_ok = true;
                        }
                        _ => issues.push("CPU boost"),
                    }
                }
            });
            rowsep(ui);
            row(ui, "GPU boost", "Graphics performance level", |ui| {
                let old_gpu = p.gpu;
                egui::ComboBox::from_id_salt(Id::new("gpu_boost").with(ac))
                    .selected_text(["Low", "Medium", "High"][p.gpu.min(2) as usize])
                    .width(156.0)
                    .show_ui(ui, |ui| {
                        for (idx, label) in
                            ["Low", "Medium", "High"].iter().enumerate()
                        {
                            ui.selectable_value(&mut p.gpu, idx as u8, *label);
                        }
                    });
                if p.gpu != old_gpu {
                    match send(comms::DaemonCommand::SetPowerMode {
                        ac, pwr: 4, cpu: p.cpu, gpu: p.gpu,
                    }) {
                        Some(comms::DaemonResponse::SetPowerMode { result: true }) => {
                            any_ok = true;
                            profile_write_ok = true;
                        }
                        _ => issues.push("GPU boost"),
                    }
                }
            });
        }
    });

    if profile_write_ok {
        app.remember_power_profile_write(is_ac);
    }

    // Cooling and lighting card
    // Pre-capture GPU temp so we can check safety inside the card closure
    // without conflicting with the &mut app.ac/bat borrow on `p`.
    let gpu_temp_snap = app.gpu.as_ref().map(|g| g.temp).unwrap_or(0);
    let fan_min = app.fan_min_rpm.max(1000);
    let fan_max = app.fan_max_rpm.max(fan_min);
    let fan_span = (fan_max - fan_min).max(0) as f32;
    let recommended_manual_rpm = (fan_min as f32 + fan_span * 0.35).round() as i32;
    let safe_min_rpm = |temp_c: i32| -> i32 {
        if temp_c >= 95 {
            fan_max
        } else if temp_c >= 90 {
            (fan_min as f32 + fan_span * 0.85).round() as i32
        } else if temp_c >= 85 {
            (fan_min as f32 + fan_span * 0.60).round() as i32
        } else if temp_c >= 80 {
            (fan_min as f32 + fan_span * 0.35).round() as i32
        } else {
            fan_min
        }
    };
    let mut fan_floor_applied: Option<i32> = None;
    let mut fan_max_forced = false;
    card(ui, "Cooling and lighting", "Fan, brightness and logo LED", |ui| {
        let p = if is_ac { &mut app.ac } else { &mut app.bat };

        row(ui, "Logo LED", "Laptop lid logo lighting behaviour", |ui| {
            let old_logo = p.logo;
            egui::ComboBox::from_id_salt(Id::new("logo_led").with(ac))
                .selected_text(["Off", "On", "Breathing"][p.logo.min(2) as usize])
                .width(156.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.logo, 0, "Off");
                    ui.selectable_value(&mut p.logo, 1, "On");
                    ui.selectable_value(&mut p.logo, 2, "Breathing");
                });
            if p.logo != old_logo {
                match send(comms::DaemonCommand::SetLogoLedState { ac, logo_state: p.logo }) {
                    Some(comms::DaemonResponse::SetLogoLedState { result: true }) => any_ok = true,
                    _ => issues.push("logo LED"),
                }
            }
        });

        rowsep(ui);

        // ── Fan mode ──────────────────────────────────────────────────────
        // Three modes:
        //   0 = Auto (daemon decides)
        //   1 = Manual RPM
        //   2 = Temperature target (GUI PID controller)
        let fan_mode_idx = if p.temp_target > 0 {
            2usize
        } else if p.fan > 0 {
            1
        } else {
            0
        };
        let mut new_mode_idx = fan_mode_idx;
        row(ui, "Fan mode", "Auto · Manual RPM · Temp target", |ui| {
            egui::ComboBox::from_id_salt(Id::new("fan_mode").with(ac))
                .selected_text(["Auto", "Manual RPM", "Temp target"][fan_mode_idx])
                .width(136.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut new_mode_idx, 0, "Auto");
                    ui.selectable_value(&mut new_mode_idx, 1, "Manual RPM");
                    ui.selectable_value(&mut new_mode_idx, 2, "Temp target");
                });
        });
        if new_mode_idx != fan_mode_idx {
            match new_mode_idx {
                0 => { p.fan = 0;    p.temp_target = 0; }
                1 => { p.fan = recommended_manual_rpm; p.temp_target = 0; }
                2 => { p.fan = 0;    p.temp_target = 80; }
                _ => {}
            }
            match send(comms::DaemonCommand::SetFanSpeed { ac, rpm: p.fan }) {
                Some(comms::DaemonResponse::SetFanSpeed { result: true }) => any_ok = true,
                _ => issues.push("fan mode"),
            }
        }

        // Manual RPM slider (device-specific range)
        if new_mode_idx == 1 {
            rowsep(ui);
            let mut fan_released = false;
            row(ui, "RPM", &format!("{} – {} RPM", fan_min, fan_max), |ui| {
                fan_released = ui.add(
                    egui::Slider::new(&mut p.fan, fan_min..=fan_max)
                        .suffix(" RPM")
                        .step_by(100.0),
                ).drag_stopped();
            });
            if fan_released {
                if gpu_temp_snap >= 95 {
                    p.fan = fan_max;
                    fan_max_forced = true;
                } else {
                    let min_safe = safe_min_rpm(gpu_temp_snap);
                    if p.fan < min_safe {
                        p.fan = min_safe;
                        fan_floor_applied = Some(min_safe);
                    }
                }
                match send(comms::DaemonCommand::SetFanSpeed { ac, rpm: p.fan }) {
                    Some(comms::DaemonResponse::SetFanSpeed { result: true }) => any_ok = true,
                    _ => issues.push("fan speed"),
                }
            }
        }

        // Temperature target slider (60–95 °C) + current computed RPM readout
        if new_mode_idx == 2 {
            rowsep(ui);
            let mut target_changed = false;
            row(ui, "Max temperature", "Target °C — fan auto-adjusts to stay at or below this", |ui| {
                target_changed = ui.add(
                    egui::Slider::new(&mut p.temp_target, 60_i32..=95_i32)
                        .suffix(" °C")
                        .step_by(1.0),
                ).changed();
            });
            let _ = target_changed; // Controller handles sending; no IPC needed on target change.
            // Status readout — clearly labelled as auto-managed so the user
            // doesn't mistake the displayed RPM for the Manual-RPM mode.
            let lbl = if p.fan > 0 {
                format!("Auto-managed → {} RPM  ·  ≤ {} °C", p.fan, p.temp_target)
            } else {
                format!("Auto-managed → Idle  ·  ≤ {} °C", p.temp_target)
            };
            ui.add_space(4.0);
            ui.label(egui::RichText::new(lbl).size(11.5).color(crate::constants::DIM));
        }

        rowsep(ui);

        let mut bright_changed = false;
        row(ui, "Keyboard brightness", "Backlight level (0 = off)", |ui| {
            let avail = ui.available_width();
            let br = ui.add(
                egui::Slider::new(&mut p.bright, 0_u8..=100_u8)
                    .suffix("%")
                    .clamping(egui::SliderClamping::Always)
                    .min_decimals(0)
                    .max_decimals(0),
            );
            // Consume any leftover width so layout stays flush.
            let used = ui.min_rect().width();
            if used < avail {
                ui.add_space(avail - used);
            }
            bright_changed = br.drag_stopped() || br.lost_focus();
        });
        if bright_changed {
            match send(comms::DaemonCommand::SetBrightness { ac, val: p.bright }) {
                Some(comms::DaemonResponse::SetBrightness { result: true }) => any_ok = true,
                _ => issues.push("brightness"),
            }
        }
    });

    // Fan safety banners — emitted after the card closure so `p`'s borrow has ended.
    if fan_max_forced {
        app.set_banner(
            BannerTone::Error,
            format!("GPU at {}°C — fan set to max ({} RPM) for system safety.", gpu_temp_snap, fan_max),
        );
    } else if let Some(min_safe) = fan_floor_applied {
        app.set_banner(
            BannerTone::Warn,
            format!("GPU at {}°C — manual fan floor raised to {} RPM.", gpu_temp_snap, min_safe),
        );
    }

    // Consolidation card (AC only)
    if is_ac {
        card(
            ui,
            "Consolidation",
            "Keep AC and battery profiles aligned when needed",
            |ui| {
                let mut sync_changed = false;
                row(ui, "Sync profiles", "Mirror AC settings to battery", |ui| {
                    sync_changed = ui.checkbox(&mut app.sync, "Enabled").changed();
                });
                if sync_changed {
                    match send(comms::DaemonCommand::SetSync { sync: app.sync }) {
                        Some(comms::DaemonResponse::SetSync { result: true }) => any_ok = true,
                        _ => issues.push("sync"),
                    }
                }
            },
        );
    }

    // Post-render feedback
    if !issues.is_empty() {
        app.set_banner(
            BannerTone::Warn,
            format!(
                "Not acknowledged by daemon: {}. Is razer-daemon running as Administrator?",
                issues.join(", ")
            ),
        );
    } else if any_ok {
        app.wake_poll();
    }

    // ── System features card (both tabs) ──────────────────────────────────
    card(ui, "System features", "Keyboard, input and display tweaks", |ui| {
        // Fn Key Swap — EC HID command via daemon
        let fn_label = if app.fn_swap { "ON — media keys primary" } else { "OFF — F-keys primary" };
        row(ui, "Fn key swap", fn_label, |ui| {
            let old = app.fn_swap;
            ui.checkbox(&mut app.fn_swap, "");
            if app.fn_swap != old {
                match send(comms::DaemonCommand::SetFnSwap { swap: app.fn_swap }) {
                    Some(comms::DaemonResponse::SetFnSwap { result: true }) => {
                        app.wake_poll();
                    }
                    _ => {
                        app.fn_swap = old; // revert on failure
                        app.set_banner(
                            BannerTone::Warn,
                            "Fn key swap did not stick after write. Blade 16 likely needs the proprietary Synapse path.",
                        );
                    }
                }
            }
        });

        rowsep(ui);

        // Gaming Mode — Win key block
        row(ui, "Block Win key", "Prevent accidental Start menu", |ui| {
            let old = app.gaming_win_key;
            ui.checkbox(&mut app.gaming_win_key, "");
            if app.gaming_win_key != old {
                let result = send(comms::DaemonCommand::SetGamingMode {
                    win_key: app.gaming_win_key,
                    alt_tab: app.gaming_alt_tab,
                    alt_f4: app.gaming_alt_f4,
                });
                if !matches!(result, Some(comms::DaemonResponse::SetGamingMode { result: true })) {
                    app.gaming_win_key = old;
                    app.set_banner(BannerTone::Warn, "Gaming mode update was not acknowledged by the daemon.");
                }
                app.save_gui_config();
            }
        });
        rowsep(ui);
        row(ui, "Block Alt+Tab", "Prevent task-switching", |ui| {
            let old = app.gaming_alt_tab;
            ui.checkbox(&mut app.gaming_alt_tab, "");
            if app.gaming_alt_tab != old {
                let result = send(comms::DaemonCommand::SetGamingMode {
                    win_key: app.gaming_win_key,
                    alt_tab: app.gaming_alt_tab,
                    alt_f4: app.gaming_alt_f4,
                });
                if !matches!(result, Some(comms::DaemonResponse::SetGamingMode { result: true })) {
                    app.gaming_alt_tab = old;
                    app.set_banner(BannerTone::Warn, "Gaming mode update was not acknowledged by the daemon.");
                }
                app.save_gui_config();
            }
        });
        rowsep(ui);
        row(ui, "Block Alt+F4", "Prevent accidental close", |ui| {
            let old = app.gaming_alt_f4;
            ui.checkbox(&mut app.gaming_alt_f4, "");
            if app.gaming_alt_f4 != old {
                let result = send(comms::DaemonCommand::SetGamingMode {
                    win_key: app.gaming_win_key,
                    alt_tab: app.gaming_alt_tab,
                    alt_f4: app.gaming_alt_f4,
                });
                if !matches!(result, Some(comms::DaemonResponse::SetGamingMode { result: true })) {
                    app.gaming_alt_f4 = old;
                    app.set_banner(BannerTone::Warn, "Gaming mode update was not acknowledged by the daemon.");
                }
                app.save_gui_config();
            }
        });

        // Battery refresh rate — only on Battery tab
        if !is_ac && app.display_rates.len() >= 2 {
            rowsep(ui);
            let label = format!(
                "Switch to {} Hz on battery (AC: {} Hz)",
                app.display_rate_low, app.display_rate_high,
            );
            row(ui, "Low refresh on battery", &label, |ui| {
                let old = app.bat_low_refresh;
                ui.checkbox(&mut app.bat_low_refresh, "");
                if app.bat_low_refresh != old {
                    if !app.bat_low_refresh {
                        // Turning off: restore high refresh rate immediately.
                        crate::display::set_refresh_rate(app.display_rate_high);
                    }
                    app.save_gui_config();
                }
            });
        }

        // Low-battery lighting auto-off — only on Battery tab
        if !is_ac {
            rowsep(ui);
            let old_low_bat = (app.low_bat_lighting, app.low_bat_pct);
            row(ui, "Dim on low battery", "Turn off KB + logo when battery is low", |ui| {
                ui.checkbox(&mut app.low_bat_lighting, "");
            });
            if app.low_bat_lighting {
                rowsep(ui);
                row(ui, "Threshold", "Battery percentage below which lighting turns off", |ui| {
                    ui.add(
                        egui::Slider::new(&mut app.low_bat_pct, 5_u8..=50_u8)
                            .suffix("%")
                            .step_by(5.0),
                    );
                });
            }
            if (app.low_bat_lighting, app.low_bat_pct) != old_low_bat {
                app.save_gui_config();
            }
        }
    });
}
