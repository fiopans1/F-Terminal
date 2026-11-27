use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::highlight::StyledSpan;

const MAX_BYTES: usize = 2 * 1024 * 1024;

/// Tiny UTF-8 text editor used by the integrated editor panel.
pub struct Editor {
    pub path: PathBuf,
    pub lines: Vec<String>,
    /// Cursor line.
    pub cursor_row: usize,
    /// Cursor position within the line, in Unicode scalar values.
    pub cursor_col: usize,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub dirty: bool,
    /// Increments whenever the buffer content changes.
    pub revision: u64,
    pub highlight: Vec<Vec<StyledSpan>>,
    pub highlight_dirty: bool,
}

impl Editor {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        if bytes.len() > MAX_BYTES {
            bail!(
                "{} is too large for the built-in editor ({} bytes)",
                path.display(),
                bytes.len()
            );
        }
        if bytes.contains(&0) {
            bail!("{} looks like a binary file", path.display());
        }

        let text = String::from_utf8(bytes)
            .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
        let text = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }

        Ok(Self {
            path: path.to_path_buf(),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            dirty: false,
            revision: 0,
            highlight: Vec::new(),
            highlight_dirty: true,
        })
    }

    pub fn save(&mut self) -> Result<()> {
        fs::write(&self.path, self.lines.join("\n"))
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.highlight_dirty = true;
    }

    pub fn char_to_byte(line: &str, char_index: usize) -> usize {
        byte_index(line, char_index)
    }

    pub fn insert_char(&mut self, character: char) {
        if character == '\n' {
            self.insert_newline();
            return;
        }
        let byte = byte_index(&self.lines[self.cursor_row], self.cursor_col);
        self.lines[self.cursor_row].insert(byte, character);
        self.cursor_col += 1;
        self.mark_dirty();
    }

    pub fn insert_str(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return;
        }

        // Fast path for text without line breaks or tabs (typing, simple paste).
        if !normalized.contains('\n') && !normalized.contains('\t') {
            let byte = byte_index(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].insert_str(byte, &normalized);
            self.cursor_col += normalized.chars().count();
            self.mark_dirty();
            return;
        }

        for character in normalized.chars() {
            if character == '\t' {
                for _ in 0..4 {
                    self.insert_char(' ');
                }
            } else {
                self.insert_char(character);
            }
        }
    }

    fn insert_newline(&mut self) {
        let byte = byte_index(&self.lines[self.cursor_row], self.cursor_col);
        let rest = self.lines[self.cursor_row].split_off(byte);
        self.lines.insert(self.cursor_row + 1, rest);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.mark_dirty();
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let byte = byte_index(&self.lines[self.cursor_row], self.cursor_col - 1);
            self.lines[self.cursor_row].remove(byte);
            self.cursor_col -= 1;
            self.mark_dirty();
        } else if self.cursor_row > 0 {
            let previous_row = self.cursor_row - 1;
            let previous_len = char_len(&self.lines[previous_row]);
            let current = self.lines.remove(self.cursor_row);
            self.lines[previous_row].push_str(&current);
            self.cursor_row = previous_row;
            self.cursor_col = previous_len;
            self.mark_dirty();
        }
    }

    pub fn delete(&mut self) {
        let line_len = char_len(&self.lines[self.cursor_row]);
        if self.cursor_col < line_len {
            let byte = byte_index(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte);
            self.mark_dirty();
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.mark_dirty();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = char_len(&self.lines[self.cursor_row]);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_col < char_len(&self.lines[self.cursor_row]) {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_row]));
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_row]));
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_col = char_len(&self.lines[self.cursor_row]);
    }

    pub fn page_up(&mut self, rows: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(rows.max(1));
        self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_row]));
    }

    pub fn page_down(&mut self, rows: usize) {
        self.cursor_row = (self.cursor_row + rows.max(1)).min(self.lines.len().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_row]));
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = col.min(char_len(&self.lines[self.cursor_row]));
    }

    /// Adjust the scroll offsets so the cursor is visible.
    pub fn ensure_visible(&mut self, rows: usize, cols: usize) {
        if rows == 0 || cols == 0 {
            return;
        }

        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        } else if self.cursor_row >= self.scroll_row + rows {
            self.scroll_row = self.cursor_row + 1 - rows;
        }

        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        } else if self.cursor_col >= self.scroll_col + cols {
            self.scroll_col = self.cursor_col + 1 - cols;
        }
    }
}

fn char_len(line: &str) -> usize {
    line.chars().count()
}

fn byte_index(line: &str, char_index: usize) -> usize {
    line.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

#[cfg(test)]
#[path = "tests/editor.rs"]
mod tests;
