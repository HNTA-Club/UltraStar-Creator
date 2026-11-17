use eframe::egui;

use crate::app::lyrics::{role_accent, role_tint};
use crate::app::{PlaybackState, format_time, is_play_enabled};
use crate::lyrics::{Role, Syllable};
use crate::tapping::{Filter, Session};

#[derive(Default)]
pub struct TapActions {
    pub toggle_play: bool,
    pub stop: bool,
    pub reset: bool,
    pub seek_s: Option<f64>,
    pub speed_changed: bool,
    pub filter_changed: Option<Filter>,
    pub tap_press: bool,
    pub tap_release: bool,
    pub undo: bool,
    pub undo_row: bool,
    pub save: bool,
}

pub fn filter_label(filter: Filter) -> &'static str {
    match filter {
        Filter::All => "All Singers",
        Filter::P1AndBoth => "P1 + Both",
        Filter::P2AndBoth => "P2 + Both",
        Filter::P1Only => "P1 Only",
        Filter::P2Only => "P2 Only",
        Filter::BothOnly => "Both Only",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: PlaybackState,
    bass_ok: bool,
    has_audio: bool,
    is_duet: bool,
    pos_s: f64,
    duration_s: f64,
    speed_pct: &mut i32,
    filter: Filter,
    session: Option<&Session>,
    mouse_tapping: &mut bool,
    output_text: &str,
    output_font_size: &mut f32,
) -> TapActions {
    let mut actions = TapActions::default();

    ui.heading("Tapping & UltraStar File");
    ui.add_space(4.0);

    let play_enabled = bass_ok && has_audio && is_play_enabled(state);
    let is_active = matches!(state, PlaybackState::Playing | PlaybackState::Paused);
    let can_reset = has_audio
        && matches!(
            state,
            PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Initialized
        );

    ui.horizontal(|ui| {
        let play_lbl = if state == PlaybackState::Playing {
            "Pause (Ctrl+P)"
        } else {
            "Play (Ctrl+P)"
        };
        if ui
            .add_enabled(play_enabled, egui::Button::new(play_lbl))
            .clicked()
        {
            actions.toggle_play = true;
        }
        if ui
            .add_enabled(is_active, egui::Button::new("Stop (S)"))
            .clicked()
        {
            actions.stop = true;
        }
        if ui
            .add_enabled(can_reset, egui::Button::new("Reset (Ctrl+R)"))
            .clicked()
        {
            actions.reset = true;
        }
    });

    ui.horizontal(|ui| {
        let mut pos = pos_s.clamp(0.0, duration_s.max(0.0));
        ui.label(format_time(pos));
        let slider = egui::Slider::new(&mut pos, 0.0..=duration_s.max(0.01))
            .show_value(false)
            .trailing_fill(true);
        if ui.add_enabled(is_active && bass_ok, slider).changed() {
            actions.seek_s = Some(pos);
        }
        ui.label(format!("-{}", format_time((duration_s - pos).max(0.0))));
    });

    ui.horizontal(|ui| {
        ui.label("Speed:");
        if ui
            .add_enabled(
                has_audio && bass_ok,
                egui::Slider::new(speed_pct, 50..=150).suffix("%"),
            )
            .changed()
        {
            actions.speed_changed = true;
        }
    });

    if is_duet {
        let can_change_filter =
            matches!(state, PlaybackState::Initialized | PlaybackState::Stopped);
        ui.horizontal(|ui| {
            ui.label("Tap filter:");
            ui.add_enabled_ui(can_change_filter, |ui| {
                let mut selected = filter;
                egui::ComboBox::from_id_salt("tap_filter_combo")
                    .selected_text(filter_label(selected))
                    .show_ui(ui, |ui| {
                        for f in [
                            Filter::All,
                            Filter::P1AndBoth,
                            Filter::P2AndBoth,
                            Filter::P1Only,
                            Filter::P2Only,
                            Filter::BothOnly,
                        ] {
                            if ui.selectable_value(&mut selected, f, filter_label(f)).changed() {
                                actions.filter_changed = Some(selected);
                            }
                        }
                    });
            });
        });
    }

    ui.separator();

    // Tap button + Next 1..5 preview
    let dark = ui.visuals().dark_mode;
    let current_syl = session.and_then(|s| s.current().map(|idx| &s.syllables()[idx]));
    let upcoming: Vec<&Syllable> = session
        .map(|s| {
            let syls = s.syllables();
            s.remaining_queue()
                .iter()
                .skip(1)
                .take(5)
                .map(|&idx| &syls[idx])
                .collect()
        })
        .unwrap_or_default();

    let tap_enabled = state == PlaybackState::Playing && current_syl.is_some();
    let tap_text = current_syl
        .map(|s| display_syllable(&s.text, s.line_end))
        .unwrap_or_else(|| "Tap (Space / V)".into());

    let tap_role = current_syl.map_or(Role::Solo, |s| s.role);
    let mut tap_btn = egui::Button::new(egui::RichText::new(tap_text).size(18.0).strong())
        .min_size(egui::vec2(ui.available_width().max(120.0), 46.0));
    if is_duet && tap_enabled {
        tap_btn = tap_btn
            .fill(role_tint(tap_role, dark))
            .stroke(egui::Stroke::new(2.5_f32, role_accent(tap_role)));
    }

    let tap_resp = ui.add_enabled(tap_enabled, tap_btn);
    if tap_enabled {
        let down = tap_resp.is_pointer_button_down_on();
        if down && !*mouse_tapping {
            *mouse_tapping = true;
            actions.tap_press = true;
        } else if !down && *mouse_tapping {
            *mouse_tapping = false;
            actions.tap_release = true;
        }
    } else if *mouse_tapping {
        *mouse_tapping = false;
    }

    ui.horizontal_wrapped(|ui| {
        for (i, syl) in upcoming.iter().enumerate() {
            let txt = display_syllable(&syl.text, syl.line_end);
            let mut btn = egui::Button::new(txt);
            if is_duet {
                btn = btn
                    .fill(role_tint(syl.role, dark))
                    .stroke(egui::Stroke::new(1.5_f32, role_accent(syl.role)));
            }
            ui.add_enabled(tap_enabled, btn)
                .on_disabled_hover_text(format!("Next {}", i + 1));
        }
    });

    let can_undo = is_active && session.is_some_and(|s| s.progress().0 > 0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_undo, egui::Button::new("Undo Tap (X)"))
            .clicked()
        {
            actions.undo = true;
        }
        if ui
            .add_enabled(can_undo, egui::Button::new("Undo Row (Z)"))
            .clicked()
        {
            actions.undo_row = true;
        }
    });

    ui.separator();

    ui.horizontal(|ui| {
        if ui
            .add_enabled(has_audio && !output_text.is_empty(), egui::Button::new("Save .txt (Ctrl+S)"))
            .clicked()
        {
            actions.save = true;
        }
        if ui.button("A-").clicked() {
            *output_font_size = (*output_font_size - 1.0).max(8.0);
        }
        if ui.button("A+").clicked() {
            *output_font_size = (*output_font_size + 1.0).min(24.0);
        }
    });

    ui.add_space(4.0);
    let avail = ui.available_size();
    let mut readonly_buf = output_text;
    egui::ScrollArea::vertical()
        .id_salt("output_scroll")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut readonly_buf)
                    .font(egui::FontId::monospace(*output_font_size))
                    .interactive(state != PlaybackState::Playing)
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(avail.x, (avail.y - 8.0).max(160.0))),
            );
        });

    actions
}

fn display_syllable(text: &str, line_end: bool) -> String {
    let base = text.replace(' ', "␣");
    if line_end {
        format!("{base} ↵")
    } else {
        base
    }
}
