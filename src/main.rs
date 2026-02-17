use color_eyre::eyre::{Context, OptionExt};
use log::debug;
use std::{
    collections::VecDeque,
    io::{Read, Write},
};

use cvt::cvt;
use libc::{
    ECHO, ICANON, ISIG, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, TCSAFLUSH, TCSANOW, TIOCGWINSZ,
};

use crate::logger::setup_logger;

mod logger;

#[allow(dead_code)]
#[derive(Debug)]
struct WindowSize {
    col: u16,
    row: u16,
}

#[derive(Debug)]
struct GapBuffer {
    // sort of
    start: Vec<char>,
    end: VecDeque<char>,
}

#[derive(Debug)]
enum Mode {
    Normal,
    Insertion,
}

#[derive(Debug)]
struct State {
    previous_io_settings: libc::termios,
    current_io_settings: libc::termios,
    stdout: std::io::Stdout,
    window_size: WindowSize,
    cursor_pos: WindowSize,
    text_lines: Vec<String>,
    edited_line: Option<GapBuffer>,
    current_mode: Mode,
}

const STARTING_COL: u16 = 3;

impl Drop for State {
    fn drop(&mut self) {
        let mut lock = self.stdout.lock();
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
                    col: window_size.ws_col,
                    row: window_size.ws_row,
                });
            }
        }
    }

    None
}

impl State {
    #[allow(dead_code)]
    fn get_current_line(&self) -> Option<&str> {
        self.text_lines
            .get(self.cursor_pos.row as usize)
            .map(|line| line.as_str())
    }

    fn get_current_line_mut(&mut self) -> Option<&mut String> {
        self.text_lines.get_mut(self.cursor_pos.row as usize)
    }

    fn init_ui(&mut self) -> color_eyre::Result<()> {
        let mut lock = self.stdout.lock();
        // Enable alt buffer, move cursor to 0,0, move cursor right STARTING_COL columns
        term_write!(&mut lock, "\x1b[?1049h\x1b[H\x1b[{STARTING_COL}C")?;

        self.draw_ui()
    }

    fn draw_ui(&mut self) -> color_eyre::Result<()> {
        let mut lock = self.stdout.lock();
        // Save cursor pos, clear screen, move cursor to 0,0
        term_write!(&mut lock, "\x1b7\x1b[2J\x1b[H")?;

        for n_line in 0..self.window_size.row - 2 {
            term_write!(&mut lock, "~  ")?;

            let is_cursor_line = n_line == self.cursor_pos.row;
            let is_insertion = matches!(self.current_mode, Mode::Insertion);

            if is_cursor_line {
                // Set highlight color
                term_write!(&mut lock, "\x1b[48;2;54;58;79m")?;
            }

            match (is_cursor_line, is_insertion, &self.edited_line) {
                (true, true, Some(gap)) => {
                    for c in gap.start.iter().chain(&gap.end) {
                        term_write!(&mut lock, "{c}")?;
                    }
                }
                _ => {
                    term_write!(
                        &mut lock,
                        "{}",
                        self.text_lines
                            .get(n_line as usize)
                            .map_or("", |line| line.as_str())
                    )?;
                }
            }

            // Erase in line, reset all modes, move cursor to beginning of next line
            term_write!(&mut lock, "\x1b[K\x1b[0m\x1b[1E")?;
        }

        // Set background color and erase it in line
        term_write!(
            &mut lock,
            "\x1b[48;2;30;32;48m This is the overlay\x1b[K\x1b[0m",
        )?;

        // Restore saved cursor pos + set blinking mode
        term_write!(&mut lock, "\x1b8\x1b[25m")?;

        flush(&mut lock)
    }

    /// Returns true if the program should continue
    fn handle_keypress_normal(&mut self, c: u8) -> color_eyre::Result<bool> {
        let mut stdout_lock = self.stdout.lock();

        match c {
            b'h' => {
                if self.cursor_pos.col == 0 {
                    return Ok(true);
                }
                self.cursor_pos.col -= 1;
                term_write!(&mut stdout_lock, "\x1b[1D")?;
            }
            b'j' => {
                if self.cursor_pos.row >= self.window_size.row - 3 {
                    return Ok(true);
                }
                self.cursor_pos.row += 1;
                term_write!(&mut stdout_lock, "\x1b[1B")?;
            }
            b'k' => {
                if self.cursor_pos.row == 0 {
                    return Ok(true);
                }
                self.cursor_pos.row -= 1;
                term_write!(&mut stdout_lock, "\x1b[1A")?;
            }
            b'l' => {
                if self.cursor_pos.col >= self.window_size.col - 1 - STARTING_COL {
                    return Ok(true);
                }
                self.cursor_pos.col += 1;
                term_write!(&mut stdout_lock, "\x1b[1C")?;
            }
            b'd' => {
                if let Some(line) = self.get_current_line_mut() {
                    line.clear();
                }
            }
            b'i' => {
                // This is kinda weird but whatever
                self.edited_line = if let Some(line) = self.get_current_line()
                    && (self.cursor_pos.col as usize) < line.len()
                {
                    let mut gap_buffer = GapBuffer {
                        start: Vec::with_capacity(self.cursor_pos.col as usize * 2),
                        end: line[self.cursor_pos.col as usize..].chars().collect(),
                    };
                    gap_buffer
                        .start
                        .extend(line[..self.cursor_pos.col as usize].chars());

                    Some(gap_buffer)
                } else {
                    None
                };

                if self.edited_line.is_some() {
                    self.current_mode = Mode::Insertion;
                }
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
}

fn main() -> color_eyre::Result<()> {
    setup_logger()?;

    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    cvt(unsafe { libc::tcgetattr(STDIN_FILENO, &raw mut termios) })
        .wrap_err("Could not get terminal parameters")?;

    let mut state = State {
        previous_io_settings: termios,
        current_io_settings: termios,
        stdout: std::io::stdout(),
        window_size: get_window_size().ok_or_eyre("Could not get window size")?,
        cursor_pos: WindowSize { col: 0, row: 0 },
        text_lines: vec![
            "#include <stdio.h>".to_string(),
            "".to_string(),
            "int main(void) {".to_string(),
            "    return 0;".to_string(),
            "}".to_string(),
        ],
        edited_line: None,
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

        let should_exit = match state.current_mode {
            Mode::Normal => !state
                .handle_keypress_normal(c)
                .wrap_err("Error while handling keypress [NORMAL]")?,
            Mode::Insertion => {
                let mut stdout_lock = state.stdout.lock();

                if c == 27 {
                    // ESC
                    state.current_mode = Mode::Normal;

                    if let Some(mut gap) = state.edited_line.take()
                        && let Some(line) = state.get_current_line_mut() {
                            line.reserve(gap.start.len() + gap.end.len());
                            line.clear();
                            line.extend(gap.start.drain(..));
                            line.extend(gap.end.drain(..));
                        }
                } else if c == 127 {
                    // BACKSPACE
                    if state.cursor_pos.col != 0
                        && let Some(gap) = &mut state.edited_line
                        && gap.start.pop().is_some() {
                            state.cursor_pos.col -= 1;
                            term_write!(&mut stdout_lock, "\x1b[1D")?;
                        }
                } else {
                    // TODO: check end of window
                    if let Some(gap) = &mut state.edited_line {
                        gap.start.push(c as char);
                        state.cursor_pos.col += 1;
                        term_write!(&mut stdout_lock, "\x1b[1C")?;
                    }
                }

                false
            }
        };

        if should_exit {
            break;
        }

        state.draw_ui().wrap_err("Failed to draw UI")?;
    }

    Ok(())
}
