use eframe::egui;
use egui::text::{LayoutJob, LayoutSection, TextFormat};

use crate::app::{PreviewState, format_time};
use crate::lyrics::Role;
use crate::romaji::{HatsuonMode, LongVowelMode, RomajiConfig};
use crate::tapping::Session;

pub const fn role_tag(role: Role) -> &'static str {
    match role {
        Role::P1 => "[P1]",
        Role::P2 => "[P2]",
        Role::Both => "[Both]",
        Role::Solo => "",
    }
}

pub const fn role_accent(role: Role) -> egui::Color32 {
    match role {
        Role::P1 => egui::Color32::from_rgb(37, 99, 235),
        Role::P2 => egui::Color32::from_rgb(220, 38, 38),
        Role::Both => egui::Color32::from_rgb(217, 119, 6),
        Role::Solo => egui::Color32::from_rgb(100, 116, 139),
    }
}

pub const fn role_tint(role: Role, dark: bool) -> egui::Color32 {
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
    pub split_jp_romaji: bool,
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
    active_session: Option<&Session>,
    romaji_cfg: &mut RomajiConfig,
) -> LyricsActions {
    let mut actions = LyricsActions::default();

    if locked && let Some(session) = active_session {
        show_teleprompter(ui, session, is_duet, font_size);
        return actions;
    }

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
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!locked, |ui| {
            if ui.button("Open .txt...").clicked() {
                actions.open_file = true;
            }
            if ui.button("Paste").clicked() {
                actions.paste = true;
            }
            ui.separator();
            if ui
                .button("Split Syllables")
                .on_hover_text(
                    "Split lyrics into syllables using Song Language (Japanese/Romaji mora splitter for Japanese, dictionary for other languages)",
                )
                .clicked()
            {
                actions.syllabify = true;
            }
            ui.menu_button("Options ▾", |ui| {
                ui.label(egui::RichText::new("Japanese / Romaji Split Options").strong());
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Split char:");
                    ui.selectable_value(&mut romaji_cfg.split_char, '•', "• (Bullet)");
                    ui.selectable_value(&mut romaji_cfg.split_char, '+', "+ (Plus)");
                });
                ui.horizontal(|ui| {
                    ui.label("Long vowels:");
                    ui.selectable_value(
                        &mut romaji_cfg.long_vowels,
                        LongVowelMode::Ascii,
                        "ASCII (tou)",
                    );
                    ui.selectable_value(
                        &mut romaji_cfg.long_vowels,
                        LongVowelMode::Double,
                        "Double (too)",
                    );
                    ui.selectable_value(
                        &mut romaji_cfg.long_vowels,
                        LongVowelMode::Macron,
                        "Macron (tō)",
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Hatsuon (ん):");
                    ui.selectable_value(
                        &mut romaji_cfg.hatsuon,
                        HatsuonMode::Attach,
                        "Attach (shin)",
                    );
                    ui.selectable_value(
                        &mut romaji_cfg.hatsuon,
                        HatsuonMode::Standalone,
                        "Standalone (shi•n)",
                    );
                });
                ui.checkbox(
                    &mut romaji_cfg.merge_diphthongs,
                    "Merge diphthongs (tai•you, se•kai)",
                );
                ui.checkbox(&mut romaji_cfg.strict_mora, "Strict mora (split ー)");
                ui.separator();
                if ui
                    .button("Split as Japanese / Romaji")
                    .on_hover_text(
                        "Force Japanese / Romaji mora splitting even if Song Language isn't set to Japanese yet",
                    )
                    .clicked()
                {
                    actions.split_jp_romaji = true;
                    ui.close();
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("A+").clicked() {
                *font_size = (*font_size + 1.0).min(24.0);
            }
            if ui.button("A-").clicked() {
                *font_size = (*font_size - 1.0).max(8.0);
            }
        });
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

fn show_teleprompter(
    ui: &mut egui::Ui,
    session: &Session,
    is_duet: bool,
    font_size: &mut f32,
) {
    ui.horizontal(|ui| {
        ui.heading("Tapping Teleprompter");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("A+").clicked() {
                *font_size = (*font_size + 1.0).min(24.0);
            }
            if ui.button("A-").clicked() {
                *font_size = (*font_size - 1.0).max(8.0);
            }
        });
    });
    ui.separator();
    ui.add_space(12.0);

    let (lines, active_line) = session.queued_lines();
    let syls = session.syllables();
    let current_syl = session.current();
    let is_pressed = session.is_pressed();
    let dark = ui.visuals().dark_mode;
    let prompter_px = (*font_size + 7.0).clamp(16.0, 32.0);
    let font_id = egui::FontId::proportional(prompter_px);

    let slots: [(Option<usize>, bool, &str); 4] = [
        (active_line.checked_sub(1), false, "Previous"),
        (Some(active_line), true, "Current"),
        (Some(active_line + 1), false, "Next"),
        (Some(active_line + 2), false, "Upcoming"),
    ];

    let wrap_width = (ui.available_width() - 32.0).max(120.0);

    for (line_idx_opt, is_current_row, slot_label) in slots {
        let row_syls = line_idx_opt.and_then(|idx| lines.get(idx));
        let row_role = row_syls
            .and_then(|indices| indices.first())
            .and_then(|&i| syls.get(i))
            .map_or(Role::Solo, |s| s.role);

        let frame_fill = if is_current_row {
            if is_duet && row_role != Role::Solo {
                role_tint(row_role, dark)
            } else if dark {
                egui::Color32::from_rgb(30, 41, 59)
            } else {
                egui::Color32::from_rgb(241, 245, 249)
            }
        } else {
            egui::Color32::TRANSPARENT
        };

        let frame_stroke = if is_current_row {
            let border_color = if is_duet && row_role != Role::Solo {
                role_accent(row_role)
            } else if dark {
                egui::Color32::from_rgb(71, 85, 105)
            } else {
                egui::Color32::from_rgb(148, 163, 184)
            };
            egui::Stroke::new(1.5_f32, border_color)
        } else {
            egui::Stroke::NONE
        };

        egui::Frame::new()
            .fill(frame_fill)
            .stroke(frame_stroke)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(prompter_px * 1.6);

                let Some(indices) = row_syls else {
                    ui.colored_label(
                        egui::Color32::from_gray(if dark { 75 } else { 180 }),
                        egui::RichText::new("—").size(prompter_px),
                    );
                    return;
                };

                let job = build_teleprompter_row_job(
                    indices,
                    syls,
                    is_current_row,
                    current_syl,
                    is_pressed,
                    is_duet,
                    row_role,
                    dark,
                    font_id.clone(),
                    wrap_width,
                );
                ui.label(job).on_hover_text(slot_label);
            });

        ui.add_space(8.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_teleprompter_row_job(
    indices: &[usize],
    syls: &[crate::lyrics::Syllable],
    is_current_row: bool,
    current_syl: Option<usize>,
    is_pressed: bool,
    is_duet: bool,
    row_role: Role,
    dark: bool,
    font_id: egui::FontId,
    wrap_width: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let muted_fg = if dark {
        egui::Color32::from_rgb(135, 148, 168)
    } else {
        egui::Color32::from_rgb(120, 132, 150)
    };
    let bullet_fg = if dark {
        egui::Color32::from_rgb(95, 108, 128)
    } else {
        egui::Color32::from_rgb(160, 172, 188)
    };
    let active_row_fg = if dark {
        egui::Color32::from_rgb(248, 250, 252)
    } else {
        egui::Color32::from_rgb(15, 23, 42)
    };

    if is_duet && row_role != Role::Solo {
        let tag = format!("{} ", role_tag(row_role));
        let start = job.text.len();
        job.text.push_str(&tag);
        let badge_color = if is_current_row {
            role_accent(row_role)
        } else {
            muted_fg
        };
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: start..job.text.len(),
            format: TextFormat {
                font_id: font_id.clone(),
                color: badge_color,
                ..Default::default()
            },
        });
    }

    for (row_pos, &syl_idx) in indices.iter().enumerate() {
        let Some(syl) = syls.get(syl_idx) else {
            continue;
        };

        let (color, background) = if !is_current_row {
            (muted_fg, egui::Color32::TRANSPARENT)
        } else if Some(syl_idx) == current_syl {
            let bg = if is_pressed {
                egui::Color32::from_rgb(22, 163, 74)
            } else {
                egui::Color32::from_rgb(37, 99, 235)
            };
            (egui::Color32::WHITE, bg)
        } else if current_syl.is_some_and(|cur| syl_idx < cur) {
            (muted_fg, egui::Color32::TRANSPARENT)
        } else {
            (active_row_fg, egui::Color32::TRANSPARENT)
        };

        let trimmed = syl.text.trim_end_matches(' ');
        let trailing_spaces = &syl.text[trimmed.len()..];

        if !trimmed.is_empty() {
            let start = job.text.len();
            job.text.push_str(trimmed);
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: start..job.text.len(),
                format: TextFormat {
                    font_id: font_id.clone(),
                    color,
                    background,
                    ..Default::default()
                },
            });
        }

        if !trailing_spaces.is_empty() {
            let start = job.text.len();
            job.text.push_str(trailing_spaces);
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: start..job.text.len(),
                format: TextFormat {
                    font_id: font_id.clone(),
                    color: if is_current_row { active_row_fg } else { muted_fg },
                    background: egui::Color32::TRANSPARENT,
                    ..Default::default()
                },
            });
        } else if row_pos + 1 < indices.len() && !trimmed.ends_with('-') {
            let start = job.text.len();
            job.text.push('•');
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: start..job.text.len(),
                format: TextFormat {
                    font_id: font_id.clone(),
                    color: bullet_fg,
                    background: egui::Color32::TRANSPARENT,
                    ..Default::default()
                },
            });
        }
    }

    job
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

    #[test]
    fn teleprompter_renders_dimmed_bullets_for_plus_and_bullet_splits() {
        let syls = crate::lyrics::syllables("The ma+gi+cal po•tion", false);
        let indices: Vec<usize> = (0..syls.len()).collect();
        let job = build_teleprompter_row_job(
            &indices,
            &syls,
            true,
            Some(0),
            false,
            false,
            Role::Solo,
            true,
            egui::FontId::proportional(20.0),
            500.0,
        );
        assert_eq!(job.text, "The ma•gi•cal po•tion");
        // Section 0 is "The" (highlighted), Section 1 is " " (transparent background)
        assert_ne!(job.sections[0].format.background, egui::Color32::TRANSPARENT);
        assert_eq!(job.sections[1].format.background, egui::Color32::TRANSPARENT);
    }
}
