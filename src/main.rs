use color_eyre::eyre::{Context, OptionExt};
use log::debug;
use std::{
    collections::VecDeque,
    io::{Read, Write, stdout},
};
use unicode_width::UnicodeWidthChar;

use cvt::cvt;
use libc::{
    ECHO, ICANON, ISIG, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, TCSAFLUSH, TCSANOW, TIOCGWINSZ,
};

use crate::{line::Line, logger::setup_logger};

mod line;
mod logger;

#[derive(Debug)]
struct WindowSize {
    col: usize,
    row: usize,
}

#[derive(Debug)]
struct SplitBuffer {
    start: Vec<char>,
    end: VecDeque<char>,
}

#[derive(Debug)]
enum Mode {
    Normal,
    Insertion { buffer: SplitBuffer },
}

#[derive(Debug)]
struct State {
    previous_io_settings: libc::termios,
    current_io_settings: libc::termios,
    window_size: WindowSize,
    cursor_pos: WindowSize,
    target_col: usize,
    text_lines: Vec<Line>,
    current_mode: Mode,
}

const STARTING_COL: usize = 3;

impl Drop for State {
    fn drop(&mut self) {
        let mut lock = stdout().lock();
        // Disable alt buffer
        let _ = lock.write(b"\x1b[?1049l");
        unsafe {
            libc::tcsetattr(STDIN_FILENO, TCSANOW, &raw const self.previous_io_settings);
        }
    }
}

macro_rules! term_write {
    ($lock:expr, $($arg:tt)*) => {{
        use std::io::Write;
        write!($lock, $($arg)*)
            .wrap_err("Could not write to stdout")
    }};
}

fn flush(lock: &mut std::io::StdoutLock) -> color_eyre::Result<()> {
    lock.flush().wrap_err("Failed to flush stdout")
}

fn get_window_size() -> Option<WindowSize> {
    let mut window_size: libc::winsize;
    unsafe {
        window_size = std::mem::zeroed();

        for stream in [STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO] {
            let error = libc::ioctl(stream, TIOCGWINSZ, &raw mut window_size);
            if error == 0 {
                return Some(WindowSize {
                    col: window_size.ws_col as usize,
                    row: window_size.ws_row as usize,
                });
            }
        }
    }

    None
}

impl State {
    fn get_current_line(&self) -> Option<&Line> {
        self.text_lines.get(self.cursor_pos.row)
    }

    fn get_current_line_mut(&mut self) -> Option<&mut Line> {
        self.text_lines.get_mut(self.cursor_pos.row)
    }

    fn init_ui(&mut self) -> color_eyre::Result<()> {
        let mut lock = stdout().lock();
        // Enable alt buffer
        term_write!(&mut lock, "\x1b[?1049h")?;

        self.draw_ui()
    }

    fn draw_ui(&mut self) -> color_eyre::Result<()> {
        let mut lock = stdout().lock();
        // Clear screen, move cursor to 0,0
        term_write!(&mut lock, "\x1b[2J\x1b[H")?;

        for n_line in 0..self.window_size.row - 2 {
            term_write!(&mut lock, "~  ")?;

            let is_cursor_line = n_line == self.cursor_pos.row;

            if is_cursor_line {
                // Set highlight color
                term_write!(&mut lock, "\x1b[48;2;54;58;79m")?;
            }

            if is_cursor_line && let Mode::Insertion { buffer } = &self.current_mode {
                for c in buffer.start.iter().chain(&buffer.end) {
                    term_write!(&mut lock, "{c}")?;
                }
            } else {
                term_write!(
                    &mut lock,
                    "{}",
                    self.text_lines.get(n_line).map_or("", |line| line.as_str())
                )?;
            }

            // Erase in line, reset all modes, move cursor to beginning of next line
            term_write!(&mut lock, "\x1b[K\x1b[0m\x1b[1E")?;
        }

        // Set background color and erase it in line
        term_write!(
            &mut lock,
            "\x1b[48;2;30;32;48m This is the overlay\x1b[K\x1b[0m",
        )?;

        let columns = if let Mode::Insertion { buffer } = &self.current_mode {
            buffer.start.iter().map(|&c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
        } else if let Some(line) = self.get_current_line() {
            line.get_unicode_width_at(self.cursor_pos.col)
        } else {
            self.cursor_pos.col
        };

        // Move cursor to its position, set blinking mode
        // NB: apparently the escape code used to position the cursor
        // is 1 indexed so we need to add 1
        term_write!(
            &mut lock,
            "\x1b[{};{}H\x1b[25m",
            self.cursor_pos.row + 1,
            columns + STARTING_COL + 1
        )?;

        flush(&mut lock)
    }

    fn clamp_col_to_current_line(&mut self) {
        let len = self.get_current_line().map_or(0, |l| l.len());
        self.cursor_pos.col = self.target_col.min(len);
    }

    fn enable_insertion_mode(&mut self) {
        if let Some(line) = self.get_current_line()
            && self.cursor_pos.col <= line.len()
        {
            let mut split_buffer = SplitBuffer {
                // Those are completely arbitrary values
                // This may need more extensive testing to find better ones
                start: Vec::with_capacity(30.max(self.cursor_pos.col * 3).max(line.len() * 2)),
                end: line.chars().skip(self.cursor_pos.col).collect(),
            };
            split_buffer
                .start
                .extend(line.chars().take(self.cursor_pos.col));

            self.current_mode = Mode::Insertion {
                buffer: split_buffer,
            };
        }
    }

    // Maybe we don't need Result anymore as nothing returns an error
    /// Returns true if the program should continue
    fn handle_keypress_normal(&mut self, c: u8) -> color_eyre::Result<bool> {
        match c {
            b'h' => {
                if self.cursor_pos.col == 0 {
                    return Ok(true);
                }
                self.cursor_pos.col -= 1;
                self.target_col = self.cursor_pos.col;
            }
            b'l' => {
                if let Some(line) = self.get_current_line()
                    && self.cursor_pos.col >= line.len()
                {
                    return Ok(true);
                }
                self.cursor_pos.col += 1;
                self.target_col = self.cursor_pos.col;
            }
            b'j' => {
                if self.cursor_pos.row >= self.window_size.row - 3
                    || self.cursor_pos.row >= self.text_lines.len() - 1
                {
                    return Ok(true);
                }
                self.cursor_pos.row += 1;
                self.clamp_col_to_current_line();
            }
            b'k' => {
                if self.cursor_pos.row == 0 {
                    return Ok(true);
                }
                self.cursor_pos.row -= 1;
                self.clamp_col_to_current_line();
            }
            b'd' => {
                if let Some(line) = self.get_current_line_mut() {
                    line.clear();
                    let lines_below = &mut self.text_lines[self.cursor_pos.row..];
                    lines_below.rotate_left(1);

                    // This is not how Vim does it but whatever for now
                    self.clamp_col_to_current_line();
                }
            }
            b'i' => {
                self.enable_insertion_mode();
            }
            b'o' => {
                self.cursor_pos.row += 1;
                if self.cursor_pos.row == self.text_lines.len() {
                    // This should not allocate yet so this is good
                    self.text_lines.push(Line::new());
                } else {
                    // Just else because it is assumed the cursor cannot be out of bounds
                    // This assumption is only true if I know how to code correctly
                    self.text_lines.insert(self.cursor_pos.row, Line::new());
                }
                self.cursor_pos.col = 0;
                self.enable_insertion_mode();
            }
            b'q' => {
                return Ok(false);
            }

            _ => {
                debug!("{c}");
            }
        }

        Ok(true)
    }

    /// Returns true if the program should continue
    fn handle_keypress_insertion(
        &mut self,
        c: u8,
        mut buffer: SplitBuffer,
    ) -> color_eyre::Result<bool> {
        let mut stdout_lock = stdout().lock();

        if c == 27 {
            // ESC
            self.current_mode = Mode::Normal;
            self.target_col = self.cursor_pos.col;

            if let Some(line) = self.get_current_line_mut() {
                line.reserve(buffer.start.len() + buffer.end.len());
                line.clear();
                line.extend(buffer.start.drain(..));
                line.extend(buffer.end.drain(..));
            }

            return Ok(true);
        } else if c == 127 {
            // BACKSPACE
            if self.cursor_pos.col != 0 && buffer.start.pop().is_some() {
                self.cursor_pos.col -= 1;
                term_write!(&mut stdout_lock, "\x1b[1D")?;
            }
        } else if c.is_ascii_graphic() || c == b' ' {
            // TODO: check end of window
            buffer.start.push(c as char);
            self.cursor_pos.col += 1;
            term_write!(&mut stdout_lock, "\x1b[1C")?;
        }

        self.current_mode = Mode::Insertion { buffer };
        Ok(true)
    }
}

fn main() -> color_eyre::Result<()> {
    setup_logger()?;

    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    cvt(unsafe { libc::tcgetattr(STDIN_FILENO, &raw mut termios) })
        .wrap_err("Could not get terminal parameters")?;

    let mut state = State {
        previous_io_settings: termios,
        current_io_settings: termios,
        window_size: get_window_size().ok_or_eyre("Could not get window size")?,
        cursor_pos: WindowSize { col: 0, row: 0 },
        target_col: 0,
        text_lines: vec![
            Line::with_string("#include <stdio.h>".to_string()),
            Line::with_string("".to_string()),
            Line::with_string("int main(void) {".to_string()),
            Line::with_string("    printf(\"%s\\n\", \"🍆\");".to_string()),
            Line::with_string("    return 0;".to_string()),
            Line::with_string("}".to_string()),
        ],
        current_mode: Mode::Normal,
    };

    // TODO: use cfmakeraw instead
    state.current_io_settings.c_lflag &= !(ECHO | ICANON | ISIG);

    cvt(unsafe {
        libc::tcsetattr(
            STDIN_FILENO,
            TCSAFLUSH,
            &raw const state.current_io_settings,
        )
    })
    .wrap_err("Could not set terminal parameters")?;

    let mut buffer = [0u8; 1];

    state.init_ui().wrap_err("Failed to initialize UI")?;

    let mut stdin_lock = std::io::stdin().lock();
    loop {
        stdin_lock
            .read_exact(&mut buffer)
            .wrap_err("Could not read character from standard input")?;

        let c = buffer[0];

        let current_mode = std::mem::replace(&mut state.current_mode, Mode::Normal);

        let should_exit = !match current_mode {
            Mode::Normal => state
                .handle_keypress_normal(c)
                .wrap_err("Error while handling keypress [NORMAL]")?,
            Mode::Insertion { buffer } => state
                .handle_keypress_insertion(c, buffer)
                .wrap_err("Error while handling keypress [INSERTION]")?,
        };

        if should_exit {
            break;
        }

        state.draw_ui().wrap_err("Failed to draw UI")?;
    }

    Ok(())
}
