use color_eyre::eyre::{Context, OptionExt};
use log::debug;
use std::io::{Read, Write};

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
struct State {
    previous_io_settings: libc::termios,
    current_io_settings: libc::termios,
    stdout: std::io::Stdout,
    window_size: WindowSize,
    cursor_pos: WindowSize,
    text_lines: Vec<String>,
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

fn init_ui(state: &mut State) -> color_eyre::Result<()> {
    let mut lock = state.stdout.lock();
    // Enable alt buffer, move cursor to 0,0, move cursor right 2 columns
    term_write!(&mut lock, "\x1b[?1049h\x1b[H\x1b[{STARTING_COL}C")?;

    draw_ui(state)
}

fn draw_ui(state: &mut State) -> color_eyre::Result<()> {
    debug!("UI redrawn");

    let mut lock = state.stdout.lock();
    // Save cursor pos, clear screen, move cursor to 0,0
    term_write!(&mut lock, "\x1b7\x1b[2J\x1b[H")?;

    for n_line in 0..state.window_size.row - 2 {
        // Write ~ , go down 1 line, go left 2 columns
        term_write!(
            &mut lock,
            "~  {}\x1b[1E",
            state
                .text_lines
                .get(n_line as usize)
                .map_or("", |line| line.as_str())
        )?;
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
        cursor_pos: WindowSize {
            col: STARTING_COL,
            row: 0,
        },
        text_lines: vec![
            "#include <stdio.h>".to_string(),
            "".to_string(),
            "int main(void) {".to_string(),
            "    return 0;".to_string(),
            "}".to_string(),
        ],
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

    init_ui(&mut state).wrap_err("Failed to initialize UI")?;

    let mut stdin_lock = std::io::stdin().lock();
    loop {
        stdin_lock
            .read_exact(&mut buffer)
            .wrap_err("Could not read character from standard input")?;

        let mut stdout_lock = state.stdout.lock();

        let c = buffer[0];
        match c {
            b'h' => {
                if state.cursor_pos.col <= STARTING_COL {
                    continue;
                }
                state.cursor_pos.col -= 1;
                term_write!(&mut stdout_lock, "\x1b[1D")?;
            }
            b'j' => {
                if state.cursor_pos.row >= state.window_size.row - 3 {
                    continue;
                }
                state.cursor_pos.row += 1;
                term_write!(&mut stdout_lock, "\x1b[1B")?;
            }
            b'k' => {
                if state.cursor_pos.row == 0 {
                    continue;
                }
                state.cursor_pos.row -= 1;
                term_write!(&mut stdout_lock, "\x1b[1A")?;
            }
            b'l' => {
                if state.cursor_pos.col >= state.window_size.col - 1 {
                    continue;
                }
                state.cursor_pos.col += 1;
                term_write!(&mut stdout_lock, "\x1b[1C")?;
            }
            b'q' => {
                break;
            }

            _ => {}
        }

        draw_ui(&mut state).wrap_err("Failed to draw UI")?;
    }

    Ok(())
}
