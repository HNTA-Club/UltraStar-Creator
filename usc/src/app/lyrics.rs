use eframe::egui;
use egui::text::{LayoutJob, LayoutSection, TextFormat};

use crate::app::{PreviewState, format_time};
use crate::lyrics::Role;

pub fn role_tag(role: Role) -> &'static str {
    match role {
        Role::P1 => "[P1]",
        Role::P2 => "[P2]",
        Role::Both => "[Both]",
        Role::Solo => "",
    }
}

pub fn role_accent(role: Role) -> egui::Color32 {
    match role {
        Role::P1 => egui::Color32::from_rgb(37, 99, 235),
        Role::P2 => egui::Color32::from_rgb(220, 38, 38),
        Role::Both => egui::Color32::from_rgb(217, 119, 6),
        Role::Solo => egui::Color32::from_rgb(100, 116, 139),
    }
}

pub fn role_tint(role: Role, dark: bool) -> egui::Color32 {
    match (role, dark) {
        (Role::P1, false) => egui::Color32::from_rgb(219, 234, 254),
        (Role::P1, true) => egui::Color32::from_rgb(30, 58, 95),
        (Role::P2, false) => egui::Color32::from_rgb(254, 226, 226),
        (Role::P2, true) => egui::Color32::from_rgb(92, 29, 36),
        (Role::Both, false) => egui::Color32::from_rgb(254, 243, 199),
        (Role::Both, true) => egui::Color32::from_rgb(85, 52, 10),
        (Role::Solo, false) => egui::Color32::TRANSPARENT,
        (Role::Solo, true) => egui::Color32::TRANSPARENT,
    }
}

pub fn parse_line_role(line: &str) -> (Role, &str) {
    let trimmed = line.trim_start();
    let Some(after_open) = trimmed.strip_prefix('[') else {
        return (Role::Solo, line);
    };
    let Some(close_idx) = after_open.find(']') else {
        return (Role::Solo, line);
    };
    let tag = &after_open[..close_idx];
    let role = if tag.eq_ignore_ascii_case("p1") {
        Role::P1
    } else if tag.eq_ignore_ascii_case("p2") {
        Role::P2
    } else if tag.eq_ignore_ascii_case("both") || tag.eq_ignore_ascii_case("all") {
        Role::Both
    } else {
        return (Role::Solo, line);
    };
    (role, after_open[close_idx + 1..].trim_start())
}

pub fn toggle_role_on_lines(
    text: &str,
    char_range: (usize, usize),
    role: Role,
) -> (String, (usize, usize)) {
    let (sel_start, sel_end) = (char_range.0.min(char_range.1), char_range.0.max(char_range.1));
    let lines: Vec<&str> = text.split('\n').collect();

    let mut line_bounds = Vec::with_capacity(lines.len());
    let mut cursor = 0usize;
    for line in &lines {
        let len = line.chars().count();
        line_bounds.push((cursor, cursor + len));
        cursor += len + 1;
    }

    let find_line = |char_idx: usize| -> usize {
        line_bounds
            .iter()
            .position(|&(_, end)| char_idx <= end)
            .unwrap_or_else(|| lines.len().saturating_sub(1))
    };

    let start_line = find_line(sel_start);
    let end_line = if sel_end > sel_start {
        find_line(sel_end - 1).max(start_line)
    } else {
        start_line
    };

    let tag = role_tag(role);
    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut new_sel_start = 0usize;
    let mut new_sel_end = 0usize;
    let mut out_chars = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        let updated = if idx >= start_line && idx <= end_line {
            let (existing, stripped) = parse_line_role(line);
            if existing == role || tag.is_empty() {
                stripped.to_string()
            } else if stripped.is_empty() {
                tag.to_string()
            } else {
                format!("{tag} {stripped}")
            }
        } else {
            (*line).to_string()
        };

        let len = updated.chars().count();
        if idx == start_line {
            new_sel_start = out_chars;
        }
        if idx == end_line {
            new_sel_end = out_chars + len;
        }
        out_chars += len + 1;
        out_lines.push(updated);
    }

    (out_lines.join("\n"), (new_sel_start, new_sel_end))
}

#[derive(Default)]
pub struct LyricsActions {
    pub open_file: bool,
    pub paste: bool,
    pub syllabify: bool,
    pub preview_toggle: bool,
    pub preview_seek_s: Option<f64>,
    pub text_changed: bool,
    pub assign_role: Option<Role>,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    lyrics_text: &mut String,
    font_size: &mut f32,
    cursor_range: &mut (usize, usize),
    pending_cursor: &mut Option<(usize, usize)>,
    is_duet: bool,
    locked: bool,
    has_audio: bool,
    bass_ok: bool,
    preview_state: PreviewState,
    preview_pos_s: f64,
    duration_s: f64,
) -> LyricsActions {
    let mut actions = LyricsActions::default();

    ui.heading("Song Lyrics");
    ui.add_space(4.0);

    // Audio preview bar
    ui.horizontal(|ui| {
        let preview_enabled = has_audio && bass_ok && !locked;
        let btn_label = match preview_state {
            PreviewState::Playing => "Pause Preview",
            _ => "Preview",
        };
        if ui
            .add_enabled(preview_enabled, egui::Button::new(btn_label))
            .clicked()
        {
            actions.preview_toggle = true;
        }

        let mut pos = preview_pos_s.clamp(0.0, duration_s.max(0.0));
        ui.label(format_time(pos));
        let slider = egui::Slider::new(&mut pos, 0.0..=duration_s.max(0.01))
            .show_value(false)
            .trailing_fill(true);
        if ui.add_enabled(preview_enabled, slider).changed() {
            actions.preview_seek_s = Some(pos);
        }
        ui.label(format!("-{}", format_time((duration_s - pos).max(0.0))));
    });

    ui.separator();

    // Lyrics toolbar
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!locked, |ui| {
            if ui.button("Open .txt...").clicked() {
                actions.open_file = true;
            }
            if ui.button("Paste").clicked() {
                actions.paste = true;
            }
            if ui.button("Syllabify").clicked() {
                actions.syllabify = true;
            }
        });
        if ui.button("A-").clicked() {
            *font_size = (*font_size - 1.0).max(8.0);
        }
        if ui.button("A+").clicked() {
            *font_size = (*font_size + 1.0).min(24.0);
        }
    });

    if is_duet {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!locked, |ui| {
                ui.label("Assign line role:");
                if role_button(ui, "P1 (Alt+1)", Role::P1).clicked() {
                    actions.assign_role = Some(Role::P1);
                }
                if role_button(ui, "P2 (Alt+2)", Role::P2).clicked() {
                    actions.assign_role = Some(Role::P2);
                }
                if role_button(ui, "Both (Alt+3)", Role::Both).clicked() {
                    actions.assign_role = Some(Role::Both);
                }
            });
        });
    }

    ui.add_space(4.0);

    let size = *font_size;
    let dark = ui.visuals().dark_mode;
    let default_fg = ui.visuals().text_color();

    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = build_lyrics_layout(text.as_str(), wrap_width, size, is_duet, dark, default_fg);
        ui.fonts_mut(|f| f.layout_job(job))
    };

    let avail = ui.available_size();
    egui::ScrollArea::vertical()
        .id_salt("lyrics_scroll")
        .show(ui, |ui| {
            let mut editor = egui::TextEdit::multiline(lyrics_text)
                .id(egui::Id::new("input_lyrics_editor"))
                .desired_width(f32::INFINITY)
                .min_size(egui::vec2(avail.x, (avail.y - 8.0).max(200.0)))
                .interactive(!locked)
                .layouter(&mut layouter);

            if let Some((start, end)) = pending_cursor.take() {
                let id = egui::Id::new("input_lyrics_editor");
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    let c_start = egui::text::CCursor::new(start);
                    let c_end = egui::text::CCursor::new(end);
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(c_start, c_end)));
                    state.store(ui.ctx(), id);
                }
                editor = editor.lock_focus(true);
            }

            let output = editor.show(ui);
            if output.response.changed() {
                actions.text_changed = true;
            }
            if let Some(range) = output.cursor_range {
                let a = range.primary.index;
                let b = range.secondary.index;
                *cursor_range = (a.min(b), a.max(b));
            }
        });

    actions
}

fn role_button(ui: &mut egui::Ui, label: &str, role: Role) -> egui::Response {
    let accent = role_accent(role);
    ui.add(egui::Button::new(egui::RichText::new(label).color(accent)))
}

fn build_lyrics_layout(
    text: &str,
    wrap_width: f32,
    font_size: f32,
    is_duet: bool,
    dark: bool,
    default_fg: egui::Color32,
) -> LayoutJob {
    let font_id = egui::FontId::proportional(font_size);
    let mut job = LayoutJob {
        text: text.to_owned(),
        ..Default::default()
    };
    job.wrap.max_width = wrap_width;

    if !is_duet {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: 0..text.len(),
            format: TextFormat {
                font_id,
                color: default_fg,
                ..Default::default()
            },
        });
        return job;
    }

    let line_fg = if dark {
        egui::Color32::from_rgb(248, 250, 252)
    } else {
        egui::Color32::from_rgb(30, 41, 59)
    };

    let mut offset = 0usize;
    for chunk in text.split_inclusive('\n') {
        let end = offset + chunk.len();
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let (role, _) = parse_line_role(line);

        if role == Role::Solo {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: offset..end,
                format: TextFormat {
                    font_id: font_id.clone(),
                    color: default_fg,
                    ..Default::default()
                },
            });
        } else {
            let bg = role_tint(role, dark);
            let accent = role_accent(role);
            let tag_end_in_line = line
                .find(']')
                .map_or(0, |i| i + 1)
                .min(line.len());

            if tag_end_in_line > 0 {
                job.sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: offset..offset + tag_end_in_line,
                    format: TextFormat {
                        font_id: font_id.clone(),
                        color: accent,
                        background: bg,
                        ..Default::default()
                    },
                });
            }
            if offset + tag_end_in_line < end {
                job.sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: offset + tag_end_in_line..end,
                    format: TextFormat {
                        font_id: font_id.clone(),
                        color: line_fg,
                        background: bg,
                        ..Default::default()
                    },
                });
            }
        }
        offset = end;
    }

    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_role_single_and_multi_line() {
        let (out1, sel1) = toggle_role_on_lines("hello\nworld", (1, 1), Role::P1);
        assert_eq!(out1, "[P1] hello\nworld");
        assert_eq!(sel1, (0, 10));

        let (out2, sel2) = toggle_role_on_lines(&out1, sel1, Role::P1);
        assert_eq!(out2, "hello\nworld");
        assert_eq!(sel2, (0, 5));

        let (out3, sel3) = toggle_role_on_lines("[P1] a\nb", (2, 8), Role::P2);
        assert_eq!(out3, "[P2] a\n[P2] b");
        assert_eq!(sel3, (0, 13));

        let (out4, _) = toggle_role_on_lines("Été\nGröße", (1, 6), Role::Both);
        assert_eq!(out4, "[Both] Été\n[Both] Größe");
    }
}
