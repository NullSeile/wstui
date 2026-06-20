use log::{error, info};
use ratatui::crossterm::event::KeyCode;
use ratatui_textarea::CursorMove;
use rfd::FileDialog;
use whatsrust as wr;

use crate::app::App;
use crate::app::events::{AppEvent, AppInput};
use crate::key_handler::Key;
use crate::vim;
use strum::{EnumIter, IntoEnumIterator};

#[derive(Debug, Copy, Clone, EnumIter)]
pub enum Operator {
    Delete,
    Change,
    Yank,
}

pub fn op_keys(op: &Operator) -> Vec<Key> {
    match op {
        Operator::Delete => vec![Key::c('d')],
        Operator::Change => vec![Key::c('c')],
        Operator::Yank => vec![Key::c('y')],
    }
}

impl App<'_> {
    pub fn set_vim_mode(&mut self, mode: vim::Mode) {
        if mode != vim::Mode::VisualLine {
            self.visual_line_anchor = None;
        }
        self.vim.mode = mode;
        self.input_border = mode.block();
        self.input_widget.set_cursor_style(mode.cursor_style());
    }

    pub fn input_on_event(&mut self, key: &Key) {
        if self.kh.kp(&[Key::ctrl('x')]) {
            if let Some(c) = self.get_selected_chat() {
                let text = self.input_widget.lines().join("\n");
                let msg = if let Some((path, typ)) = &self.attached_file {
                    wr::MessageContent::File(wr::FileContent {
                        kind: typ.clone(),
                        path: path.clone(),
                        file_id: "".into(),
                        caption: Some(text.into()),
                    })
                } else {
                    wr::MessageContent::Text(text.into())
                };

                wr::send_message(&c, &msg, self.quoting_message.as_ref());

                self.input_widget.select_all();
                self.input_widget.delete_next_char();
                self.quoting_message = None;
                self.attached_file = None;
            }
            return;
        } else if self.kh.kp(&[Key::ctrl('e')]) {
            self.tx
                .send(AppInput::App(AppEvent::EditWithExternalEditor))
                .unwrap();
        }

        if self.vim.mode == vim::Mode::Normal {
            self.input_normal_on_event();
        } else if self.vim.mode == vim::Mode::Insert {
            self.input_insert_on_event(key);
        } else if self.vim.mode == vim::Mode::Visual {
            self.input_visual_on_event();
        } else if self.vim.mode == vim::Mode::VisualLine {
            self.input_visual_line_on_event();
        }
    }

    fn input_normal_on_event(&mut self) {
        if self.kh.kp(&[Key::c('i')]) {
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Insert);
        } else if self.kh.kp(&[Key::c('a')]) {
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Insert);
            self.input_widget.move_cursor(CursorMove::Forward);
        } else if self.kh.kp(&[Key::c('A')]) {
            self.input_widget.cancel_selection();
            self.input_widget.move_cursor(CursorMove::End);
            self.set_vim_mode(vim::Mode::Insert);
        } else if self.kh.kp(&[Key::c('I')]) {
            self.input_widget.cancel_selection();
            self.input_widget.move_cursor(CursorMove::Head);
            self.set_vim_mode(vim::Mode::Insert);
        } else if self.kh.kp(&[Key::c('o')]) {
            self.input_widget.move_cursor(CursorMove::End);
            self.input_widget.insert_newline();
            self.set_vim_mode(vim::Mode::Insert);
        } else if self.kh.kp(&[Key::c('O')]) {
            self.input_widget.move_cursor(CursorMove::Head);
            self.input_widget.insert_newline();
            self.input_widget.move_cursor(CursorMove::Up);
            self.set_vim_mode(vim::Mode::Insert);
        } else if self.kh.kp(&[Key::c('v')]) {
            self.set_vim_mode(vim::Mode::Visual);
            self.input_widget.start_selection();
        } else if self.kh.kp(&[Key::c('V')]) {
            self.set_vim_mode(vim::Mode::VisualLine);
            self.visual_line_anchor = Some(self.input_widget.cursor().0);
            self.input_widget.move_cursor(CursorMove::Head);
            self.input_widget.start_selection();
            self.input_widget.move_cursor(CursorMove::End);
        } else if self.kh.kp(&[Key::c('x')]) {
            self.input_widget.delete_next_char();
        } else if self.kh.kp(&[Key::c('D')]) {
            self.input_widget.delete_line_by_end();
        } else if self.kh.kp(&[Key::c('C')]) {
            self.input_widget.delete_line_by_end();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Insert);
        } else if self.kh.kp(&[Key::c('p')]) {
            self.input_widget.paste();
        } else if self.kh.kp(&[Key::c('u')]) {
            self.input_widget.undo();
        } else if self.kh.kp(&[Key::ctrl('r')]) {
            self.input_widget.redo();
        } else if self.kh.kp(&[Key::c(' '), Key::c('r')]) {
            self.quoting_message = None;
        } else if self.kh.kp(&[Key::c(' '), Key::c('a'), Key::c('r')]) {
            self.attached_file = None;
        } else if self.kh.kp(&[Key::c(' '), Key::c('a'), Key::c('i')]) {
            if let Some(path) = FileDialog::new().pick_file() {
                self.attached_file = Some((path.to_str().unwrap().into(), wr::FileKind::Image));
            }
        } else if self.kh.kp(&[Key::c(' '), Key::c('a'), Key::c('d')]) {
            if let Some(path) = FileDialog::new().pick_file() {
                self.attached_file = Some((path.to_str().unwrap().into(), wr::FileKind::Document));
            }
        } else if self.kh.kp(&[Key::c(' '), Key::c('p')]) {
            if let Ok(text) = self.clipboard.get_text() {
                self.input_widget.insert_str(&text);
            } else {
                error!("Failed to get text from clipboard");
            }
        }

        if self.kh.kp(&[Key::c('y'), Key::c('y')]) {
            self.input_widget.move_cursor(CursorMove::Head);
            self.input_widget.start_selection();
            let cursor = self.input_widget.cursor();
            self.input_widget.move_cursor(CursorMove::Down);
            if cursor == self.input_widget.cursor() {
                self.input_widget.move_cursor(CursorMove::End);
            }
            self.input_widget.copy();
            return;
        } else if self.kh.kp(&[Key::c('d'), Key::c('d')]) {
            self.input_widget.move_cursor(CursorMove::Head);
            self.input_widget.start_selection();
            let cursor = self.input_widget.cursor();
            self.input_widget.move_cursor(CursorMove::Down);
            if cursor == self.input_widget.cursor() {
                self.input_widget.move_cursor(CursorMove::End);
            }
            self.input_widget.cut();
            return;
        } else if self.kh.kp(&[Key::c('c'), Key::c('c')]) {
            self.input_widget.move_cursor(CursorMove::Head);
            self.input_widget.start_selection();
            let cursor = self.input_widget.cursor();
            self.input_widget.move_cursor(CursorMove::Down);
            if cursor == self.input_widget.cursor() {
                self.input_widget.move_cursor(CursorMove::End);
            }
            self.input_widget.cut();
            self.set_vim_mode(vim::Mode::Insert);
            return;
        }

        let mut operator_data = None;
        if let Some(op_data) = self.operator() {
            operator_data = Some(op_data);
            self.input_widget.start_selection();
        }

        let operator = operator_data.as_ref().map(|(op, _)| *op);

        let motion_executed = self.motion(operator);

        let text_object_handled = if !motion_executed {
            self.handle_text_object(operator)
        } else {
            false
        };

        if operator_data.is_some() && !motion_executed && !text_object_handled {
            self.input_widget.cancel_selection();
            return;
        }

        if motion_executed {
            if let Some(op) = &operator {
                match op {
                    Operator::Delete => {
                        self.input_widget.cut();
                    }
                    Operator::Change => {
                        info!("Change operator");
                        self.input_widget.cut();
                        self.set_vim_mode(vim::Mode::Insert);
                    }
                    Operator::Yank => {
                        self.input_widget.copy();
                    }
                }
            }
        }
    }

    fn input_insert_on_event(&mut self, key: &Key) {
        if self.kh.kp(&[Key::k(KeyCode::Esc)]) || self.kh.kp(&[Key::ctrl('c')]) {
            self.set_vim_mode(vim::Mode::Normal);
        }

        self.input_widget.input(key.clone());
    }

    fn input_visual_on_event(&mut self) {
        if self.kh.kp(&[Key::k(KeyCode::Esc)]) || self.kh.kp(&[Key::c('v')]) {
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Normal);
            return;
        }

        if self.kh.kp(&[Key::c('o')]) {
            if let Some((start, end)) = self.input_widget.selection_range() {
                let cursor = self.input_widget.cursor();
                let other = if cursor == start { end } else { start };
                self.input_widget.cancel_selection();
                self.input_widget
                    .move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
                self.input_widget.start_selection();
                self.input_widget
                    .move_cursor(CursorMove::Jump(other.0 as u16, other.1 as u16));
            }
            return;
        }

        let cursor_is_selection_end = self
            .input_widget
            .selection_range()
            .map(|(_, end)| self.input_widget.cursor() == end)
            .unwrap_or(false);

        if self.kh.kp(&[Key::c('y')]) {
            if cursor_is_selection_end {
                self.input_widget.move_cursor(CursorMove::Forward);
            }
            self.input_widget.copy();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Normal);
            return;
        } else if self.kh.kp(&[Key::c('d')]) {
            if cursor_is_selection_end {
                self.input_widget.move_cursor(CursorMove::Forward);
            }
            self.input_widget.cut();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Normal);
            return;
        } else if self.kh.kp(&[Key::c('c')]) {
            if cursor_is_selection_end {
                self.input_widget.move_cursor(CursorMove::Forward);
            }
            self.input_widget.cut();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Insert);
            return;
        }

        self.motion(None);
    }

    fn input_visual_line_on_event(&mut self) {
        if self.kh.kp(&[Key::k(KeyCode::Esc)]) || self.kh.kp(&[Key::c('V')]) {
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Normal);
            return;
        }

        if self.kh.kp(&[Key::c('o')]) {
            if let Some((start, end)) = self.input_widget.selection_range() {
                let cursor = self.input_widget.cursor();
                let other = if cursor == start { end } else { start };
                self.input_widget.cancel_selection();
                self.input_widget
                    .move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
                self.input_widget.start_selection();
                self.input_widget
                    .move_cursor(CursorMove::Jump(other.0 as u16, other.1 as u16));
            }
            return;
        }

        if self.visual_line_anchor.is_none() {
            self.visual_line_anchor = Some(self.input_widget.cursor().0);
        }

        if self.kh.kp(&[Key::c('y')]) {
            self.input_widget.copy();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Normal);
            return;
        } else if self.kh.kp(&[Key::c('d')]) {
            self.input_widget.cut();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Normal);
            return;
        } else if self.kh.kp(&[Key::c('c')]) {
            self.input_widget.cut();
            self.input_widget.cancel_selection();
            self.set_vim_mode(vim::Mode::Insert);
            return;
        }

        let before = self.input_widget.cursor();
        let anchor_row = self.visual_line_anchor.unwrap_or(before.0);
        self.motion(None);
        let after = self.input_widget.cursor();

        if before != after {
            self.input_widget.cancel_selection();
            self.input_widget
                .move_cursor(CursorMove::Jump(anchor_row as u16, 0));
            self.input_widget.move_cursor(CursorMove::Head);
            self.input_widget.start_selection();

            self.input_widget
                .move_cursor(CursorMove::Jump(after.0 as u16, 0));
            self.input_widget.move_cursor(CursorMove::Head);

            if after.0 < anchor_row {
                self.input_widget.move_cursor(CursorMove::Up);
            }
            self.input_widget.move_cursor(CursorMove::End);
        }
    }

    pub fn operator(&mut self) -> Option<(Operator, Vec<Key>)> {
        for op in Operator::iter() {
            if let Some(rest) = self.kh.kp_partial(&op_keys(&op)) {
                return Some((op, rest));
            }
        }
        None
    }

    pub fn op_kp(&mut self, op: Option<Operator>, expected: &[Key]) -> bool {
        let op_keys = op.as_ref().map(|op| op_keys(op)).unwrap_or_default();
        let keys = op_keys.into_iter().chain(expected.iter().cloned());
        self.kh.kp(&keys.collect::<Vec<_>>())
    }

    pub fn motion(&mut self, op: Option<Operator>) -> bool {
        if self.op_kp(op, &[Key::c('h')]) {
            self.input_widget.move_cursor(CursorMove::Back);
            return true;
        } else if self.op_kp(op, &[Key::c('l')]) {
            self.input_widget.move_cursor(CursorMove::Forward);
            return true;
        } else if self.op_kp(op, &[Key::c('j')]) {
            self.input_widget.move_cursor(CursorMove::Down);
            return true;
        } else if self.op_kp(op, &[Key::c('k')]) {
            self.input_widget.move_cursor(CursorMove::Up);
            return true;
        } else if self.op_kp(op, &[Key::c('w')]) {
            self.input_widget.move_cursor(CursorMove::WordForward);
            return true;
        } else if self.op_kp(op, &[Key::c('b')]) {
            self.input_widget.move_cursor(CursorMove::WordBack);
            return true;
        } else if self.op_kp(op, &[Key::c('e')]) {
            self.input_widget.move_cursor(CursorMove::WordEnd);
            if op.is_some() {
                self.input_widget.move_cursor(CursorMove::Forward);
            }
            return true;
        } else if self.op_kp(op, &[Key::c('^')]) {
            self.input_widget.move_cursor(CursorMove::Head);
            return true;
        } else if self.op_kp(op, &[Key::c('$')]) {
            self.input_widget.move_cursor(CursorMove::End);
            return true;
        } else if self.op_kp(op, &[Key::c('g'), Key::c('g')]) {
            self.input_widget.move_cursor(CursorMove::Top);
            return true;
        } else if self.op_kp(op, &[Key::c('G')]) {
            self.input_widget.move_cursor(CursorMove::Bottom);
            return true;
        }
        false
    }

    pub fn handle_text_object(&mut self, op: Option<Operator>) -> bool {
        let (row, col) = self.input_widget.cursor();
        let lines = self.input_widget.lines().to_vec();

        let range = if self.op_kp(op, &[Key::c('i'), Key::c('w')]) {
            self.word_range(&lines, row, col, false)
        } else if self.op_kp(op, &[Key::c('a'), Key::c('w')]) {
            self.word_range(&lines, row, col, true)
        } else if self.op_kp(op, &[Key::c('i'), Key::c('p')]) {
            self.paragraph_range(&lines, row, false)
        } else if self.op_kp(op, &[Key::c('a'), Key::c('p')]) {
            self.paragraph_range(&lines, row, true)
        } else if self.op_kp(op, &[Key::c('i'), Key::c('"')]) {
            self.quote_range(&lines, row, col, '"', false)
        } else if self.op_kp(op, &[Key::c('a'), Key::c('"')]) {
            self.quote_range(&lines, row, col, '"', true)
        } else if self.op_kp(op, &[Key::c('i'), Key::c('\'')]) {
            self.quote_range(&lines, row, col, '\'', false)
        } else if self.op_kp(op, &[Key::c('a'), Key::c('\'')]) {
            self.quote_range(&lines, row, col, '\'', true)
        } else {
            None
        };

        let Some((start, end)) = range else {
            return false;
        };

        self.apply_selection(start, end);

        if let Some(op) = op {
            match op {
                Operator::Delete => {
                    self.input_widget.cut();
                }
                Operator::Change => {
                    self.input_widget.cut();
                    self.set_vim_mode(vim::Mode::Insert);
                }
                Operator::Yank => {
                    self.input_widget.copy();
                }
            }
        }

        true
    }

    fn apply_selection(&mut self, start: (usize, usize), end: (usize, usize)) {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.input_widget.cancel_selection();
        self.input_widget
            .move_cursor(CursorMove::Jump(start.0 as u16, start.1 as u16));
        self.input_widget.start_selection();
        self.input_widget
            .move_cursor(CursorMove::Jump(end.0 as u16, end.1 as u16));
    }

    fn word_range(
        &self,
        lines: &[String],
        row: usize,
        col: usize,
        around: bool,
    ) -> Option<((usize, usize), (usize, usize))> {
        let line = lines.get(row)?;
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        if len == 0 {
            return None;
        }

        let mut idx = if col >= len {
            len.saturating_sub(1)
        } else {
            col
        };
        if !is_word_char(chars[idx]) {
            let mut right = idx;
            while right < len && !is_word_char(chars[right]) {
                right += 1;
            }
            if right < len {
                idx = right;
            } else {
                let mut left = idx;
                while left > 0 && !is_word_char(chars[left - 1]) {
                    left -= 1;
                }
                if left == 0 || !is_word_char(chars[left - 1]) {
                    return None;
                }
                idx = left - 1;
            }
        }

        let mut start = idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = idx + 1;
        while end < len && is_word_char(chars[end]) {
            end += 1;
        }

        if around {
            if end < len && chars[end].is_whitespace() {
                while end < len && chars[end].is_whitespace() {
                    end += 1;
                }
            } else if start > 0 && chars[start - 1].is_whitespace() {
                while start > 0 && chars[start - 1].is_whitespace() {
                    start -= 1;
                }
            }
        }

        Some(((row, start), (row, end)))
    }

    fn paragraph_range(
        &self,
        lines: &[String],
        row: usize,
        around: bool,
    ) -> Option<((usize, usize), (usize, usize))> {
        if row >= lines.len() {
            return None;
        }
        let is_blank = |line: &str| line.trim().is_empty();

        let mut start = row;
        while start > 0 && !is_blank(&lines[start - 1]) {
            start -= 1;
        }
        let mut end = row;
        while end + 1 < lines.len() && !is_blank(&lines[end + 1]) {
            end += 1;
        }

        if around {
            if start > 0 && is_blank(&lines[start - 1]) {
                start -= 1;
            }
            if end + 1 < lines.len() && is_blank(&lines[end + 1]) {
                end += 1;
            }
        }

        let end_col = lines[end].chars().count();
        Some(((start, 0), (end, end_col)))
    }

    fn quote_range(
        &self,
        lines: &[String],
        row: usize,
        col: usize,
        quote: char,
        around: bool,
    ) -> Option<((usize, usize), (usize, usize))> {
        let line = lines.get(row)?;
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        if len == 0 {
            return None;
        }

        let idx = if col >= len {
            len.saturating_sub(1)
        } else {
            col
        };
        let mut left = None;
        for i in (0..=idx).rev() {
            if chars[i] == quote {
                left = Some(i);
                break;
            }
        }
        let left = left?;
        let mut right = None;
        for i in (left + 1)..len {
            if chars[i] == quote {
                right = Some(i);
                break;
            }
        }
        let right = right?;

        let (start, end) = if around {
            (left, right + 1)
        } else {
            (left + 1, right)
        };

        if start > end {
            None
        } else {
            Some(((row, start), (row, end)))
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
