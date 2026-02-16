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
}

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

fn write(lock: &mut std::io::StdoutLock, buffer: &[u8]) -> color_eyre::Result<usize> {
    lock.write(buffer).wrap_err("Could not write to stdout")
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
    write(&mut lock, b"\x1b[?1049h\x1b[H\x1b[2C")?;

    draw_ui(state)
}

fn draw_ui(state: &mut State) -> color_eyre::Result<()> {
    debug!("UI redrawn");

    let mut lock = state.stdout.lock();
    // Save cursor pos, clear screen, move cursor to 0,0
    write(&mut lock, b"\x1b7\x1b[2J\x1b[H")?;

    for _ in 0..state.window_size.row - 2 {
        // Write ~ , go down 1 line, go left 2 columns
        write(&mut lock, b"~ \x1b[1B\x1b[2D")?;
    }

    // Set background color and erase it in line
    write(
        &mut lock,
        b"\x1b[48;2;30;32;48m This is the overlay\x1b[K\x1b[0m",
    )?;

    // Restore saved cursor pos + set blinking mode
    write(&mut lock, b"\x1b8\x1b[25m")?;

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
        if c == b'v' {
            println!("received v input");
        } else if c == b'h' {
            write(&mut stdout_lock, b"\x1b[1D")?;
        } else if c == b'j' {
            write(&mut stdout_lock, b"\x1b[1B")?;
        } else if c == b'k' {
            write(&mut stdout_lock, b"\x1b[1A")?;
        } else if c == b'l' {
            write(&mut stdout_lock, b"\x1b[1C")?;
        } else if c == b'q' {
            break;
        }

        draw_ui(&mut state).wrap_err("Failed to draw UI")?;
    }

    Ok(())
}
