use std::collections::BTreeMap;

use crate::lyrics::{Role, Syllable};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub syllable: usize,
    pub start_ms: f64,
    pub dur_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    P1AndBoth,
    P2AndBoth,
    P1Only,
    P2Only,
    BothOnly,
}

impl Filter {
    fn includes(self, role: Role) -> bool {
        matches!(
            (self, role),
            (_, Role::Solo)
                | (Filter::All, _)
                | (Filter::P1AndBoth, Role::P1 | Role::Both)
                | (Filter::P2AndBoth, Role::P2 | Role::Both)
                | (Filter::P1Only, Role::P1)
                | (Filter::P2Only, Role::P2)
                | (Filter::BothOnly, Role::Both)
        )
    }
}

pub struct Session {
    syllables: Vec<Syllable>,
    queue: Vec<usize>,
    pos: usize,
    notes: BTreeMap<usize, Note>,
    pressed: Option<f64>,
}

impl Session {
    pub fn new(syllables: Vec<Syllable>, filter: Filter, earlier: Vec<Note>) -> Session {
        let queue = syllables
            .iter()
            .enumerate()
            .filter_map(|(i, s)| filter.includes(s.role).then_some(i))
            .collect();
        let notes = earlier.into_iter().map(|n| (n.syllable, n)).collect();
        Session {
            syllables,
            queue,
            pos: 0,
            notes,
            pressed: None,
        }
    }

    pub fn press(&mut self, t_ms: f64) {
        if self.pressed.is_some() || self.pos >= self.queue.len() {
            return;
        }
        self.pressed = Some(t_ms);
    }

    pub fn release(&mut self, t_ms: f64) -> bool {
        let Some(start_ms) = self.pressed.take() else {
            return false;
        };
        if self.pos >= self.queue.len() {
            return false;
        }
        let syllable = self.queue[self.pos];
        self.notes.insert(
            syllable,
            Note {
                syllable,
                start_ms,
                dur_ms: (t_ms - start_ms).max(0.0),
            },
        );
        self.pos += 1;
        self.pos == self.queue.len()
    }

    pub fn undo(&mut self) -> f64 {
        self.pressed = None;
        if self.pos == 0 {
            return 0.0;
        }
        self.pos -= 1;
        let syllable = self.queue[self.pos];
        let removed = self.notes.remove(&syllable);
        if self.pos == 0 {
            return 0.0;
        }
        removed.map_or(0.0, |n| (n.start_ms - 1000.0).max(0.0))
    }

    pub fn undo_row(&mut self) -> f64 {
        self.pressed = None;
        if self.pos == 0 {
            return 0.0;
        }
        let mut start = self.pos - 1;
        while start > 0
            && !(self.queue[start - 1]..self.queue[start])
                .any(|idx| self.syllables[idx].line_end)
        {
            start -= 1;
        }

        let mut earliest: Option<f64> = None;
        for &syl_idx in &self.queue[start..self.pos] {
            if let Some(removed) = self.notes.remove(&syl_idx) {
                earliest = Some(match earliest {
                    Some(prev) => prev.min(removed.start_ms),
                    None => removed.start_ms,
                });
            }
        }
        self.pos = start;

        if self.pos == 0 {
            return 0.0;
        }
        earliest.map_or(0.0, |t| (t - 2000.0).max(0.0))
    }

    pub fn current(&self) -> Option<usize> {
        self.queue.get(self.pos).copied()
    }

    pub fn notes(&self) -> Vec<Note> {
        self.notes.values().copied().collect()
    }

    pub fn syllables(&self) -> &[Syllable] {
        &self.syllables
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.pos, self.queue.len())
    }

    pub fn remaining_queue(&self) -> &[usize] {
        &self.queue[self.pos..]
    }

    pub fn is_pressed(&self) -> bool {
        self.pressed.is_some()
    }

    pub fn next_syllable(&self) -> Option<usize> {
        self.queue.get(self.pos + 1).copied()
    }

    pub fn queued_lines(&self) -> (Vec<Vec<usize>>, usize) {
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut current_line: Vec<usize> = Vec::new();
        let mut active_line = 0usize;

        for (q_idx, &syl_idx) in self.queue.iter().enumerate() {
            if q_idx == self.pos {
                active_line = lines.len();
            }
            current_line.push(syl_idx);
            let line_ended = if let Some(&next_syl_idx) = self.queue.get(q_idx + 1) {
                (syl_idx..next_syl_idx).any(|i| self.syllables[i].line_end)
            } else {
                true
            };
            if line_ended {
                lines.push(std::mem::take(&mut current_line));
            }
        }
        if self.pos >= self.queue.len() {
            active_line = lines.len();
        }
        (lines, active_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solo(n_per_line: &[usize]) -> Vec<Syllable> {
        let mut out = Vec::new();
        let mut idx = 0;
        for &count in n_per_line {
            for j in 0..count {
                out.push(Syllable {
                    text: format!("s{idx}"),
                    role: Role::Solo,
                    line_end: j + 1 == count,
                });
                idx += 1;
            }
        }
        out
    }

    #[test]
    fn taps_record_notes() {
        let mut s = Session::new(solo(&[3]), Filter::All, vec![]);
        s.press(1000.0);
        assert!(!s.release(1200.0));
        s.press(1300.0);
        s.release(1400.0);
        assert_eq!(
            s.notes(),
            vec![
                Note { syllable: 0, start_ms: 1000.0, dur_ms: 200.0 },
                Note { syllable: 1, start_ms: 1300.0, dur_ms: 100.0 },
            ]
        );
        assert_eq!(s.current(), Some(2));
        s.press(1500.0);
        assert!(s.release(1600.0));
        assert_eq!(s.current(), None);
    }

    #[test]
    fn undo_seeks_one_second_before() {
        let mut s = Session::new(solo(&[3]), Filter::All, vec![]);
        s.press(1000.0);
        s.release(1200.0);
        s.press(1300.0);
        s.release(1400.0);
        assert_eq!(s.undo(), 300.0);
        assert_eq!(s.notes().len(), 1);
        assert_eq!(s.undo(), 0.0);
        assert!(s.notes().is_empty());
        assert_eq!(s.undo(), 0.0);
    }

    #[test]
    fn undo_row_back_to_line_start() {
        let mut s = Session::new(solo(&[2, 2]), Filter::All, vec![]);
        for t in [1000.0, 2000.0, 3000.0] {
            s.press(t);
            s.release(t + 100.0);
        }
        assert_eq!(s.undo_row(), 1000.0);
        assert_eq!(s.current(), Some(2));
        for t in [3000.0, 4000.0] {
            s.press(t);
            s.release(t + 100.0);
        }
        assert_eq!(s.undo_row(), 1000.0);
        assert_eq!(s.notes().len(), 2);
    }

    #[test]
    fn undo_cancels_held_press() {
        let mut s = Session::new(solo(&[3]), Filter::All, vec![]);
        s.press(1000.0);
        s.release(1100.0);
        s.press(1500.0);
        s.undo();
        assert!(!s.release(1600.0));
        assert!(s.notes().is_empty());
    }

    #[test]
    fn ignores_repeat_and_stray_events() {
        let mut s = Session::new(solo(&[1]), Filter::All, vec![]);
        assert!(!s.release(500.0));
        s.press(1000.0);
        s.press(1050.0);
        assert!(s.release(1200.0));
        s.press(1300.0);
        assert!(!s.release(1400.0));
        assert_eq!(
            s.notes(),
            vec![Note { syllable: 0, start_ms: 1000.0, dur_ms: 200.0 }]
        );
        let mut empty = Session::new(vec![], Filter::All, vec![]);
        empty.press(0.0);
        assert!(!empty.release(1.0));
        assert_eq!(empty.undo_row(), 0.0);
    }

    #[test]
    fn filter_and_two_passes() {
        let syl = vec![
            Syllable { text: "x".into(), role: Role::P1, line_end: true },
            Syllable { text: "y".into(), role: Role::P2, line_end: true },
            Syllable { text: "z".into(), role: Role::Both, line_end: true },
        ];
        let mut p1 = Session::new(syl.clone(), Filter::P1Only, vec![]);
        p1.press(1000.0);
        assert!(p1.release(1100.0));
        let mut p2 = Session::new(syl, Filter::P2AndBoth, p1.notes());
        assert_eq!(p2.current(), Some(1));
        p2.press(2000.0);
        p2.release(2100.0);
        p2.press(3000.0);
        p2.release(3100.0);
        assert_eq!(
            p2.notes().iter().map(|n| n.syllable).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn undo_row_filtered_stops_at_line_boundary() {
        let syl = vec![
            Syllable { text: "a".into(), role: Role::P1, line_end: false },
            Syllable { text: "b".into(), role: Role::P2, line_end: true },
            Syllable { text: "c".into(), role: Role::P1, line_end: true },
        ];
        let mut s = Session::new(syl, Filter::P1Only, vec![]);
        s.press(1000.0);
        s.release(1100.0);
        s.press(3000.0);
        s.release(3100.0);
        assert_eq!(s.undo_row(), 1000.0);
        assert_eq!(s.current(), Some(2));
        assert_eq!(s.notes().len(), 1);
    }

    #[test]
    fn progress_and_remaining_queue() {
        let mut s = Session::new(solo(&[3]), Filter::All, vec![]);
        assert_eq!(s.progress(), (0, 3));
        assert_eq!(s.remaining_queue(), &[0, 1, 2]);
        s.press(100.0);
        s.release(200.0);
        assert_eq!(s.progress(), (1, 3));
        assert_eq!(s.remaining_queue(), &[1, 2]);
        s.press(300.0);
        s.release(400.0);
        s.press(500.0);
        s.release(600.0);
        assert_eq!(s.progress(), (3, 3));
        assert!(s.remaining_queue().is_empty());
    }

    #[test]
    fn queued_lines_groups_by_line_boundaries_and_tracks_active_line() {
        let syl = vec![
            Syllable { text: "Hel".into(), role: Role::P1, line_end: false },
            Syllable { text: "lo".into(), role: Role::P1, line_end: false },
            Syllable { text: "skip".into(), role: Role::P2, line_end: true },
            Syllable { text: "world".into(), role: Role::P1, line_end: true },
            Syllable { text: "bye".into(), role: Role::P1, line_end: true },
        ];
        let mut s = Session::new(syl, Filter::P1Only, vec![]);
        let (lines, active) = s.queued_lines();
        assert_eq!(lines, vec![vec![0, 1], vec![3], vec![4]]);
        assert_eq!(active, 0);
        assert_eq!(s.current(), Some(0));
        assert_eq!(s.next_syllable(), Some(1));
        assert!(!s.is_pressed());

        s.press(100.0);
        assert!(s.is_pressed());
        s.release(200.0);
        assert!(!s.is_pressed());
        assert_eq!(s.queued_lines().1, 0);
        assert_eq!(s.current(), Some(1));
        assert_eq!(s.next_syllable(), Some(3));

        s.press(300.0);
        s.release(400.0);
        assert_eq!(s.queued_lines().1, 1);
        assert_eq!(s.current(), Some(3));
        assert_eq!(s.next_syllable(), Some(4));
    }
}
