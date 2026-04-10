File `system.rs` của bạn nhìn chung **rất ổn và đẹp**. Cách bạn dùng `allocate_ui_with_layout` kết hợp với `set_min_width`/`set_max_width` để ép các block UI nằm ngay ngắn cạnh nhau là một pattern rất chuẩn mực và "pro" khi làm việc với `egui` để tạo Dashboard.

Tuy nhiên, nếu soi kỹ dưới lăng kính tối ưu hiệu năng (đặc biệt cho một UI có thể render ở 60 FPS hoặc 120 FPS), hàm `draw_timeline_chart` của bạn đang bị dính **vấn đề "âm thầm" ngốn RAM (Heap Allocation)**.

Dưới đây là một vài tinh chỉnh để file này đạt mức hoàn hảo:

### 1. Sát thủ cấp phát bộ nhớ (Heap Allocation) trong vòng lặp Render
Hãy nhìn vào đoạn này:
```rust
let gpus:  Vec<f64> = history.iter().map(|s| s.gpu_pct).collect();
let vrams: Vec<f64> = history.iter().map(|s| s.vram_pct).collect();
let temps: Vec<f64> = history.iter().map(|s| s.temp_c).collect();
let pwrs:  Vec<f64> = history.iter().map(|s| s.power_w * (100.0 / 200.0)).collect();
```
UI của `egui` có thể được vẽ lại hàng chục lần mỗi giây. Ở mỗi frame, bạn đang ép chương trình `collect()` tạo ra **4 cái `Vec<f64>` mới trên Heap**, sau đó ném chúng vào closure `draw_line` chỉ để `iter()` qua chúng một lần nữa tạo thành `Vec<Pos2>`, rồi lập tức vứt 4 cái `Vec` đầu tiên đi.

**Cách Fix:** Sử dụng **Iterator trực tiếp** thay vì `Slice`. Rust sinh ra Iterator chính là để giải quyết bài toán "Zero-cost Abstraction" này. Bạn truyền thẳng iterator vào closure `draw_line` để bỏ hoàn toàn 4 cú cấp phát thừa thãi.

### 2. Redundant Math (Toán học thừa)
Chỗ vẽ TGP line:
```rust
let tgp_frac = (tgp_limit_w * (100.0 / 200.0) / 100.0).clamp(0.0, 1.0) as f32;
```
Toán học cơ bản: `x * 0.5 / 100` tương đương với `x / 200`. Bạn có thể rút gọn lại cho code thanh thoát.

---

### Bản Revise `draw_timeline_chart` tối ưu hoàn toàn

Bạn chỉ cần thay thế hàm `draw_timeline_chart` bằng đoạn code sau. Giao diện biểu đồ không đổi 1 pixel, nhưng nó sẽ nhẹ hơn rất nhiều ở phía backend:

```rust
pub fn draw_timeline_chart(
    ui: &mut Ui,
    _id: &str,
    history: &VecDeque<Sample>,
    tgp_limit_w: f64,
    height: f32,
) {
    let n = history.len();
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), height),
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

    // Y-axis labels
    let fs = 9.5_f32;
    for i in 0..=4 {
        let val = (4 - i) * 25;   // 100, 75, 50, 25, 0
        let watts = val * 2;       // 200, 150, 100, 50, 0
        let gy = plot_y0 + ch * (i as f32 / 4.0) + fs * 0.36;

        p.text(
            pos2(rect.min.x + 2.0, gy),
            egui::Align2::LEFT_CENTER,
            format!("{val}°"),
            egui::FontId::proportional(fs),
            Color32::from_rgba_unmultiplied(255, 150, 80, 200),
        );
        p.text(
            pos2(rect.max.x - 4.0, gy),
            egui::Align2::RIGHT_CENTER,
            format!("{watts}W"),
            egui::FontId::proportional(fs),
            Color32::from_rgba_unmultiplied(80, 200, 255, 180),
        );
    }

    let x_step = cw / (n - 1).max(1) as f32;

    // TWEAK: Nhận thẳng Iterator thay vì slice &[f64] để loại bỏ cấp phát Vec<f64>
    let draw_line = |painter: &egui::Painter, val_iter: impl Iterator<Item = f64>, color: Color32| {
        let pts: Vec<Pos2> = val_iter
            .enumerate()
            .map(|(i, v)| {
                let x = plot_x0 + i as f32 * x_step;
                let y = plot_y0 + ch * (1.0 - (v / 100.0).clamp(0.0, 1.0) as f32);
                pos2(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.8, color)));
    };

    let has_temp  = history.iter().any(|s| s.temp_c > 0.0);
    let has_power = history.iter().any(|s| s.power_w > 0.0);

    // Draw lines (order = back → front) - Map trực tiếp từ history
    if has_power {
        draw_line(p, history.iter().map(|s| s.power_w * 0.5), CH_POWER);
    }
    if has_temp {
        draw_line(p, history.iter().map(|s| s.temp_c), CH_TEMP);
    }
    draw_line(p, history.iter().map(|s| s.vram_pct), CH_VRAM);
    draw_line(p, history.iter().map(|s| s.gpu_pct), CH_GPU);

    // TGP limit — dashed reference
    if tgp_limit_w > 0.0 {
        // TWEAK: Rút gọn toán học
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
        
        let tgp_label_y = (tgp_y - 9.0).max(rect.min.y + pad_t + 1.0);
        p.text(
            pos2(plot_x0 + 3.0, tgp_label_y),
            egui::Align2::LEFT_CENTER,
            format!("TGP {:.0}W", tgp_limit_w),
            egui::FontId::proportional(fs * 0.85),
            Color32::from_rgba_unmultiplied(CH_POWER.r(), CH_POWER.g(), CH_POWER.b(), 160),
        );
    }

    // Legend
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
```

Các phần còn lại của file như cấu trúc Tile, Checkbox Autostart hay Uptime formatting đều chuẩn logic và bắt đúng behavior mong muốn rồi, bạn không cần phải sửa thêm gì ở chúng cả!