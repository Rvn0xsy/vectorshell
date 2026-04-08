use unicode_width::UnicodeWidthChar;

pub struct InputState {
    buffer: String,
    cursor_pos: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    saved_input: String,
    // Tab completion state
    completion_candidates: Vec<String>,
    completion_index: Option<usize>,
    completion_prefix: String,
    pub completion_hint: Option<String>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            completion_candidates: Vec::new(),
            completion_index: None,
            completion_prefix: String::new(),
            completion_hint: None,
        }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn cursor_pos(&self) -> usize {
        self.cursor_pos
    }

    /// Display width (columns) of buffer content before the cursor.
    /// Accounts for CJK fullwidth characters (2 columns each).
    pub fn cursor_display_col(&self) -> usize {
        self.buffer[..self.cursor_pos]
            .chars()
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum()
    }

    pub fn insert_char(&mut self, c: char) {
        self.clear_completion();
        self.buffer.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        self.clear_completion();
        if self.cursor_pos > 0 {
            let prev = self.prev_char_boundary();
            self.buffer.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
        }
    }

    pub fn delete(&mut self) {
        self.clear_completion();
        if self.cursor_pos < self.buffer.len() {
            let next = self.next_char_boundary();
            self.buffer.drain(self.cursor_pos..next);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.prev_char_boundary();
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_pos < self.buffer.len() {
            self.cursor_pos = self.next_char_boundary();
        }
    }

    pub fn home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn end(&mut self) {
        self.cursor_pos = self.buffer.len();
    }

    /// Replace the buffer contents and move cursor to end.
    pub fn set_buffer(&mut self, text: &str) {
        self.clear_completion();
        self.buffer = text.to_string();
        self.cursor_pos = self.buffer.len();
    }

    /// Set completion candidates for the current input prefix.
    pub fn set_candidates(&mut self, candidates: Vec<String>) {
        if candidates.is_empty() {
            self.clear_completion();
            return;
        }
        self.completion_prefix = self.buffer.clone();
        self.completion_candidates = candidates;
        self.completion_index = Some(0);
        self.apply_completion();
    }

    /// Cycle to next completion candidate (wraps around).
    pub fn next_completion(&mut self) {
        if self.completion_candidates.is_empty() {
            return;
        }
        let idx = match self.completion_index {
            Some(i) => (i + 1) % self.completion_candidates.len(),
            None => 0,
        };
        self.completion_index = Some(idx);
        self.apply_completion();
    }

    /// Clear completion state.
    pub fn clear_completion(&mut self) {
        self.completion_candidates.clear();
        self.completion_index = None;
        self.completion_prefix.clear();
        self.completion_hint = None;
    }

    /// Returns true if we are currently in tab-completion mode.
    pub fn is_completing(&self) -> bool {
        self.completion_index.is_some() && !self.completion_candidates.is_empty()
    }

    fn apply_completion(&mut self) {
        if let Some(idx) = self.completion_index {
            if let Some(candidate) = self.completion_candidates.get(idx) {
                self.buffer = candidate.clone();
                self.cursor_pos = self.buffer.len();
                // Show hint with candidate count
                let total = self.completion_candidates.len();
                if total > 1 {
                    self.completion_hint = Some(format!("[{}/{}]", idx + 1, total));
                } else {
                    self.completion_hint = None;
                }
            }
        }
    }

    pub fn submit(&mut self) -> String {
        self.clear_completion();
        let input = self.buffer.clone();
        if !input.trim().is_empty() {
            self.history.push(input.clone());
        }
        self.buffer.clear();
        self.cursor_pos = 0;
        self.history_index = None;
        self.saved_input.clear();
        input
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.saved_input = self.buffer.clone();
                self.history_index = Some(self.history.len() - 1);
            }
            Some(0) => return,
            Some(idx) => {
                self.history_index = Some(idx - 1);
            }
        }
        if let Some(idx) = self.history_index {
            self.buffer = self.history[idx].clone();
            self.cursor_pos = self.buffer.len();
        }
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            None => return,
            Some(idx) => {
                if idx + 1 >= self.history.len() {
                    self.history_index = None;
                    self.buffer = self.saved_input.clone();
                    self.cursor_pos = self.buffer.len();
                    return;
                }
                self.history_index = Some(idx + 1);
                self.buffer = self.history[idx + 1].clone();
                self.cursor_pos = self.buffer.len();
            }
        }
    }

    fn prev_char_boundary(&self) -> usize {
        let mut pos = self.cursor_pos.saturating_sub(1);
        while pos > 0 && !self.buffer.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    fn next_char_boundary(&self) -> usize {
        let mut pos = self.cursor_pos + 1;
        while pos < self.buffer.len() && !self.buffer.is_char_boundary(pos) {
            pos += 1;
        }
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_submit() {
        let mut input = InputState::new();
        input.insert_char('h');
        input.insert_char('i');
        assert_eq!(input.buffer(), "hi");
        assert_eq!(input.cursor_pos(), 2);
        let submitted = input.submit();
        assert_eq!(submitted, "hi");
        assert_eq!(input.buffer(), "");
    }

    #[test]
    fn test_backspace() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.backspace();
        assert_eq!(input.buffer(), "a");
    }

    #[test]
    fn test_cursor_movement() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        input.home();
        assert_eq!(input.cursor_pos(), 0);
        input.move_right();
        assert_eq!(input.cursor_pos(), 1);
        input.end();
        assert_eq!(input.cursor_pos(), 3);
        input.move_left();
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn test_delete() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.home();
        input.delete();
        assert_eq!(input.buffer(), "b");
    }

    #[test]
    fn test_history() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.submit();
        input.insert_char('b');
        input.submit();

        input.insert_char('c');
        input.history_up();
        assert_eq!(input.buffer(), "b");
        input.history_up();
        assert_eq!(input.buffer(), "a");
        input.history_down();
        assert_eq!(input.buffer(), "b");
        input.history_down();
        assert_eq!(input.buffer(), "c");
    }

    #[test]
    fn test_tab_completion() {
        let mut input = InputState::new();
        let candidates = vec![
            "/sessions".to_string(),
            "/select".to_string(),
        ];
        input.set_candidates(candidates);
        assert_eq!(input.buffer(), "/sessions");
        assert!(input.is_completing());
        assert!(input.completion_hint.is_some());

        input.next_completion();
        assert_eq!(input.buffer(), "/select");

        // Wraps around
        input.next_completion();
        assert_eq!(input.buffer(), "/sessions");

        // Typing clears completion
        input.insert_char('x');
        assert!(!input.is_completing());
    }

    #[test]
    fn test_empty_candidates() {
        let mut input = InputState::new();
        input.set_candidates(vec![]);
        assert!(!input.is_completing());
        assert_eq!(input.buffer(), "");
    }

    #[test]
    fn test_cjk_cursor_width() {
        let mut input = InputState::new();
        // Chinese chars are 3 bytes UTF-8 but 2 display columns
        input.insert_char('a');
        input.insert_char('你');
        input.insert_char('好');
        assert_eq!(input.buffer(), "a你好");
        assert_eq!(input.cursor_pos(), 1 + 3 + 3); // 7 bytes
        assert_eq!(input.cursor_display_col(), 1 + 2 + 2); // 5 columns

        input.move_left(); // back one char (好)
        assert_eq!(input.cursor_display_col(), 1 + 2); // 3 columns

        input.backspace(); // delete 你
        assert_eq!(input.buffer(), "a好");
        assert_eq!(input.cursor_display_col(), 1);
    }
}
