//! Self-contained vocal amplitude ("source of truth") strip for the Tapping Teleprompter.
//! Kept isolated in this file so it can be tweaked, moved, or removed with a single-line change.

use eframe::egui;

use crate::audio::WaveformPeaks;
use crate::tapping::Session;

#[derive(Clone, Copy, Debug)]
pub struct WaveformConfig {
    pub enabled: bool,
    pub lookback_s: f64,
    pub lookahead_s: f64,
}

impl Default for WaveformConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lookback_s: 1.5,
            lookahead_s: 4.5,
        }
    }
}

/// Renders a compact header control (`[x] Vocal Waveform`) for the Teleprompter title bar.
pub fn show_header_toggle(
    ui: &mut egui::Ui,
    cfg: &mut WaveformConfig,
    peaks: Option<&WaveformPeaks>,
) {
    let label = match peaks {
        Some(p) if !p.source_label.is_empty() => format!("Waveform ({})", p.source_label),
        _ => "Waveform".to_owned(),
    };
    ui.checkbox(&mut cfg.enabled, label).on_hover_text(
        "Show live vocal amplitude envelope right above the active lyric row",
    );
}

/// Renders the slim amplitude strip docked directly above the active Teleprompter row.
pub fn show_inline_strip(
    ui: &mut egui::Ui,
    cfg: &WaveformConfig,
    peaks: Option<&WaveformPeaks>,
    pos_s: f64,
    session: &Session,
    dark: bool,
) {
    if !cfg.enabled {
        return;
    }
    let Some(peaks) = peaks.filter(|p| !p.peaks.is_empty() && p.bin_ms > 0.0) else {
        return;
    };

    let height = 54.0_f32;
    let width = ui.available_width().max(120.0);
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter_at(rect);
    let bg_color = if dark {
        egui::Color32::from_rgb(15, 23, 42)
    } else {
        egui::Color32::from_rgb(226, 232, 240)
    };
    let border_color = if dark {
        egui::Color32::from_rgb(51, 65, 85)
    } else {
        egui::Color32::from_rgb(148, 163, 184)
    };

    painter.rect(
        rect,
        6.0,
        bg_color,
        egui::Stroke::new(1.0_f32, border_color),
        egui::StrokeKind::Inside,
    );

    let total_window_s = (cfg.lookback_s + cfg.lookahead_s).max(0.5);
    let start_s = pos_s - cfg.lookback_s;
    let end_s = pos_s + cfg.lookahead_s;
    let playhead_frac = (cfg.lookback_s / total_window_s) as f32;
    let playhead_x = rect.left() + rect.width() * playhead_frac;
    let center_y = rect.center().y;
    let max_half_h = (rect.height() * 0.44).max(4.0);

    let time_to_x = |t_s: f64| -> f32 {
        let frac = ((t_s - start_s) / total_window_s) as f32;
        rect.left() + frac * rect.width()
    };

    // 1. Draw tapped note spans & currently held note span as background bands
    let tapped_fill = if dark {
        egui::Color32::from_rgba_unmultiplied(37, 99, 235, 65)
    } else {
        egui::Color32::from_rgba_unmultiplied(59, 130, 246, 75)
    };
    for note in session.notes() {
        let n_start_s = note.start_ms / 1000.0;
        let n_end_s = (note.start_ms + note.dur_ms) / 1000.0;
        if n_end_s < start_s || n_start_s > end_s {
            continue;
        }
        let x0 = time_to_x(n_start_s).clamp(rect.left(), rect.right());
        let x1 = time_to_x(n_end_s).clamp(rect.left(), rect.right());
        if x1 > x0 {
            let note_rect =
                egui::Rect::from_min_max(egui::pos2(x0, rect.top() + 2.0), egui::pos2(x1, rect.bottom() - 2.0));
            painter.rect_filled(note_rect, 2.0, tapped_fill);
        }
    }

    if let Some(press_ms) = session.press_start_ms() {
        let p_start_s = press_ms / 1000.0;
        let x0 = time_to_x(p_start_s).clamp(rect.left(), playhead_x);
        let x1 = playhead_x;
        if x1 > x0 {
            let active_fill = if dark {
                egui::Color32::from_rgba_unmultiplied(22, 163, 74, 110)
            } else {
                egui::Color32::from_rgba_unmultiplied(34, 197, 94, 120)
            };
            let active_rect =
                egui::Rect::from_min_max(egui::pos2(x0, rect.top() + 2.0), egui::pos2(x1, rect.bottom() - 2.0));
            painter.rect_filled(active_rect, 2.0, active_fill);
        }
    }

    // 2. Draw center baseline
    let baseline_color = if dark {
        egui::Color32::from_rgb(51, 65, 85)
    } else {
        egui::Color32::from_rgb(203, 213, 225)
    };
    painter.line_segment(
        [
            egui::pos2(rect.left() + 2.0, center_y),
            egui::pos2(rect.right() - 2.0, center_y),
        ],
        egui::Stroke::new(1.0_f32, baseline_color),
    );

    // 3. Draw amplitude bars across the visible time window
    let past_color = if dark {
        egui::Color32::from_rgb(100, 116, 139)
    } else {
        egui::Color32::from_rgb(148, 163, 184)
    };
    let upcoming_color = if dark {
        egui::Color32::from_rgb(56, 189, 248)
    } else {
        egui::Color32::from_rgb(2, 132, 199)
    };

    let num_cols = (rect.width() / 3.0).floor().max(40.0) as usize;
    let col_w = rect.width() / num_cols as f32;

    for col in 0..num_cols {
        let col_x0 = rect.left() + col as f32 * col_w;
        let col_x1 = col_x0 + (col_w - 1.0).max(1.0);
        let col_center_x = (col_x0 + col_x1) * 0.5;

        let t0_ms = (start_s + (col as f64 / num_cols as f64) * total_window_s) * 1000.0;
        let t1_ms = (start_s + ((col + 1) as f64 / num_cols as f64) * total_window_s) * 1000.0;

        if t1_ms < 0.0 {
            continue;
        }
        let b0 = (t0_ms.max(0.0) / peaks.bin_ms).floor() as usize;
        let b1 = ((t1_ms.max(0.0) / peaks.bin_ms).ceil() as usize).max(b0 + 1);
        if b0 >= peaks.peaks.len() {
            continue;
        }
        let slice_end = b1.min(peaks.peaks.len());
        let amp = peaks.peaks[b0..slice_end]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);

        if amp < 0.015 {
            continue;
        }

        let half_h = (amp * max_half_h).max(1.5);
        let bar_color = if col_center_x <= playhead_x {
            past_color
        } else {
            upcoming_color
        };

        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(col_x0, center_y - half_h),
            egui::pos2(col_x1, center_y + half_h),
        );
        painter.rect_filled(bar_rect, 1.0, bar_color);
    }

    // 4. Draw playhead line ("NOW")
    let playhead_color = if session.is_pressed() {
        egui::Color32::from_rgb(34, 197, 94)
    } else {
        egui::Color32::from_rgb(249, 115, 22)
    };
    painter.line_segment(
        [
            egui::pos2(playhead_x, rect.top() + 1.0),
            egui::pos2(playhead_x, rect.bottom() - 1.0),
        ],
        egui::Stroke::new(2.0_f32, playhead_color),
    );
}
