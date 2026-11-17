use crate::lyrics::Role;

pub fn role_tag(role: Role) -> &'static str {
    match role {
        Role::P1 => "[P1]",
        Role::P2 => "[P2]",
        Role::Both => "[Both]",
        Role::Solo => "",
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
