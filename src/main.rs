use color_eyre::eyre::{Context, ContextCompat, OptionExt};
use log::{debug, warn};
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, Write, stdout},
    path::PathBuf,
};
use unicode_width::UnicodeWidthChar;

use cvt::cvt;
use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, TCSAFLUSH, TCSANOW, TIOCGWINSZ};

use crate::{
    command_parser::Command,
    key::{Key, SequenceParsingError, read_key},
    line::Line,
    logger::setup_logger,
};

mod command_parser;
mod key;
mod line;
mod logger;
mod utils;

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
    Command,
}

#[allow(dead_code)]
#[derive(Debug)]
enum MessageType {
    Error,
    Warning,
    Info,
}

#[derive(Debug)]
struct Message {
    msg: String,
    r#type: MessageType,
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
    command_buf: String,
    message: Message,
    save_file: Option<PathBuf>,
    dirty: bool,
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

macro_rules! write_message {
    ($lock:expr, $nb_rows:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        term_write!(
            $lock,
            "\x1b[{};{}H{}",
            $nb_rows,
            1,
            msg
        )
    }};
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

impl MessageType {
    const fn ansi_style(&self) -> &str {
        match self {
            MessageType::Error => "\x1b[1;3;31m",
            MessageType::Warning => "\x1b[0;33m",
            MessageType::Info => "",
        }
    }
}

impl Message {
    const fn has_message(&self) -> bool {
        !self.msg.is_empty()
    }

    fn clear(&mut self) {
        self.msg.clear();
    }
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

        if matches!(self.current_mode, Mode::Command) {
            write_message!(
                &mut lock,
                self.window_size.row,
                ":{}\x1b[25m",
                self.command_buf
            )?;
        } else {
            let columns = if let Mode::Insertion { buffer } = &self.current_mode {
                write_message!(
                    &mut lock,
                    self.window_size.row,
                    "\x1b[1m-- INSERT --\x1b[22m"
                )?;

                buffer
                    .start
                    .iter()
                    .map(|&c| UnicodeWidthChar::width(c).unwrap_or(0))
                    .sum()
            } else {
                if self.message.has_message() {
                    write_message!(
                        &mut lock,
                        self.window_size.row,
                        "{}{}\x1b[0m",
                        self.message.r#type.ansi_style(),
                        self.message.msg
                    )?;
                }

                if let Some(line) = self.get_current_line() {
                    line.get_unicode_width_at(self.cursor_pos.col)
                } else {
                    self.cursor_pos.col
                }
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
        }

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

            self.message.clear();
            self.current_mode = Mode::Insertion {
                buffer: split_buffer,
            };
        }
    }

    fn add_new_line(&mut self) {
        // This should not allocate yet so this is good
        // It is assumed the cursor cannot be out of bounds
        // This assumption is only true if I know how to code correctly
        self.text_lines.insert(self.cursor_pos.row, Line::new());
        self.cursor_pos.col = 0;
        self.dirty = true;
    }

    /// Returns true if the program should continue
    fn handle_keypress_normal(&mut self, key: Key) -> bool {
        match key {
            Key::ArrowLeft | Key::Char(b'h') | Key::Backspace => {
                if self.cursor_pos.col == 0 {
                    return true;
                }
                self.cursor_pos.col -= 1;
                self.target_col = self.cursor_pos.col;
            }
            Key::ArrowRight | Key::Char(b'l') => {
                if let Some(line) = self.get_current_line()
                    && self.cursor_pos.col >= line.len()
                {
                    return true;
                }
                self.cursor_pos.col += 1;
                self.target_col = self.cursor_pos.col;
            }
            Key::ArrowDown | Key::Char(b'j') | Key::Enter => {
                if self.cursor_pos.row >= self.window_size.row - 3
                    || self.cursor_pos.row >= self.text_lines.len() - 1
                {
                    return true;
                }
                self.cursor_pos.row += 1;
                self.clamp_col_to_current_line();
            }
            Key::ArrowUp | Key::Char(b'k') => {
                if self.cursor_pos.row == 0 {
                    return true;
                }
                self.cursor_pos.row -= 1;
                self.clamp_col_to_current_line();
            }
            // TODO: change this to dd
            Key::Char(b'd') => {
                if let Some(line) = self.get_current_line_mut() {
                    line.clear();
                    let lines_below = &mut self.text_lines[self.cursor_pos.row..];
                    lines_below.rotate_left(1);

                    self.dirty = true;
                    // This is not how Vim does it but whatever for now
                    self.clamp_col_to_current_line();
                }
            }
            Key::Char(b'i') => {
                self.enable_insertion_mode();
            }
            Key::Char(b'I') => {
                self.cursor_pos.col = 0;
                self.enable_insertion_mode();
            }
            Key::Char(b'A') => {
                if let Some(line) = self.get_current_line() {
                    self.cursor_pos.col = line.len();
                    self.enable_insertion_mode();
                }
            }
            Key::Char(b'o') => {
                self.cursor_pos.row += 1;
                self.add_new_line();
                self.enable_insertion_mode();
            }
            Key::Char(b'O') => {
                self.add_new_line();
                self.enable_insertion_mode();
            }
            Key::Char(b':') => {
                self.current_mode = Mode::Command;
            }
            // TODO: change this to ZZ
            Key::Char(b'Z') => {
                return false;
            }

            _ => {
                debug!("{key:?}");
            }
        }

        true
    }

    /// Returns true if the program should continue
    fn handle_keypress_insertion(&mut self, key: Key, mut buffer: SplitBuffer) -> bool {
        match key {
            Key::Char(c) => {
                // TODO: check end of window
                buffer.start.push(c as char);
                self.cursor_pos.col += 1;
                self.dirty = true;
            }
            Key::Escape => {
                self.current_mode = Mode::Normal;
                self.target_col = self.cursor_pos.col;

                if let Some(line) = self.get_current_line_mut() {
                    line.reserve(buffer.start.len() + buffer.end.len());
                    line.clear();
                    line.extend(buffer.start.drain(..));
                    line.extend(buffer.end.drain(..));
                }

                return true;
            }
            Key::Delete => {
                if buffer.end.pop_front().is_some() {
                    self.dirty = true;
                }
            }
            Key::Backspace => {
                if self.cursor_pos.col != 0 && buffer.start.pop().is_some() {
                    self.cursor_pos.col -= 1;
                    self.dirty = true;
                }
            }
            Key::Enter => {
                if let Some(line) = self.get_current_line_mut() {
                    line.reserve(buffer.start.len());
                    line.clear();
                    line.extend(buffer.start.drain(..));
                }
                self.cursor_pos.row += 1;
                self.add_new_line();
                buffer.start.clear();
            }
            Key::Tab => {
                // TODO: check end of window
                buffer.start.extend_from_slice(&[' ', ' ', ' ', ' ']);
                self.cursor_pos.col += 4;
                self.dirty = true;
            }
            _ => {}
        }

        self.current_mode = Mode::Insertion { buffer };
        true
    }

    /// Returns true if the program should continue
    fn handle_keypress_command(&mut self, key: Key) -> bool {
        match key {
            Key::Char(c) => {
                // TODO: check end of window
                self.command_buf.push(c as char);
            }
            Key::Escape => {
                self.current_mode = Mode::Normal;
                self.message.clear();
                self.command_buf.clear();

                return true;
            }
            Key::ArrowUp => todo!(),
            Key::ArrowDown => todo!(),
            Key::ArrowLeft => todo!(),
            Key::ArrowRight => todo!(),
            Key::Delete => todo!(),
            Key::Tab => todo!(),
            Key::Backspace => {
                if self.command_buf.pop().is_none() {
                    self.current_mode = Mode::Normal;
                    self.command_buf.clear();

                    return true;
                }
            }
            Key::Enter => {
                self.current_mode = Mode::Normal;
                self.message.clear();

                let res = Command::parse(&self.command_buf);
                self.command_buf.clear();

                match res {
                    Ok(cmd) => {
                        return self.handle_command(cmd);
                    }
                    Err(err) => self.handle_parse_error(err),
                }

                return true;
            }
        }
        self.current_mode = Mode::Command;

        true
    }
}

fn main() -> color_eyre::Result<()> {
    setup_logger()?;

    let mut lines: Vec<Line> = Vec::new();
    let mut filename = None;
    let mut file_info = String::with_capacity(30);
    if let Some(arg) = std::env::args_os().nth(1) {
        let path: PathBuf = arg.into();
        // TODO: make this a future or some shit
        if let Ok(f) = File::open(&path) {
            let reader = BufReader::new(&f);
            lines = reader
                .lines()
                .map(|l| Line::with_string(l.unwrap_or_default()))
                .collect();

            let metadata = f.metadata()?;
            format!(
                "\"{}\" {}L, {}B",
                path.file_name()
                    .wrap_err("Failed to read filename")?
                    .display(),
                lines.len(),
                metadata.len()
            )
            .clone_into(&mut file_info);
        }
        filename = Some(path);
    }
    if lines.is_empty() {
        lines.push(Line::new());
    }

    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    cvt(unsafe { libc::tcgetattr(STDIN_FILENO, &raw mut termios) })
        .wrap_err("Could not get terminal parameters")?;

    let mut state = State {
        previous_io_settings: termios,
        current_io_settings: termios,
        window_size: get_window_size().ok_or_eyre("Could not get window size")?,
        cursor_pos: WindowSize { col: 0, row: 0 },
        target_col: 0,
        text_lines: lines,
        current_mode: Mode::Normal,
        command_buf: String::new(),
        message: Message {
            msg: file_info,
            r#type: MessageType::Info,
        },
        save_file: filename,
        dirty: false,
    };

    unsafe {
        libc::cfmakeraw(&raw mut state.current_io_settings);
    }
    state.current_io_settings.c_cc[libc::VMIN] = 0;
    state.current_io_settings.c_cc[libc::VTIME] = 1;

    cvt(unsafe {
        libc::tcsetattr(
            STDIN_FILENO,
            TCSAFLUSH,
            &raw const state.current_io_settings,
        )
    })
    .wrap_err("Could not set terminal parameters")?;

    state.init_ui().wrap_err("Failed to initialize UI")?;

    let mut stdin_lock = std::io::stdin().lock();
    loop {
        match read_key(&mut stdin_lock) {
            Ok(key) => {
                let current_mode = std::mem::replace(&mut state.current_mode, Mode::Normal);

                // Maybe there is a way to put the handle method in the enum?
                let should_exit = !match current_mode {
                    Mode::Normal => state.handle_keypress_normal(key),
                    Mode::Insertion { buffer } => state.handle_keypress_insertion(key, buffer),
                    Mode::Command => state.handle_keypress_command(key),
                };

                if should_exit {
                    break;
                }
            }
            Err(e) => {
                if matches!(e, SequenceParsingError::NoChar) {
                    continue;
                }
                warn!("Unsupported input: {e:?}");
                "Received unsupported input".clone_into(&mut state.message.msg);
                state.message.r#type = MessageType::Warning;
            }
        }

        state.draw_ui().wrap_err("Failed to draw UI")?;
    }

    Ok(())
}
