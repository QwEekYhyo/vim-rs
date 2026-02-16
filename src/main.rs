use color_eyre::eyre::Context;
use std::io::{Read, Write};

use cvt::cvt;
use libc::{ECHO, ICANON, ISIG, STDIN_FILENO, TCSAFLUSH, TCSANOW};

#[derive(Debug)]
struct State {
    previous_io_settings: libc::termios,
    current_io_settings: libc::termios,
    stdout: std::io::Stdout,
    displayed_lines: u32,
}

impl Drop for State {
    fn drop(&mut self) {
        let mut lock = self.stdout.lock();
        let _ = lock.write(b"\x1b[?1049l");
        unsafe {
            libc::tcsetattr(STDIN_FILENO, TCSANOW, &raw const self.previous_io_settings);
        }
    }
}

fn init_ui(state: &mut State) -> color_eyre::Result<()> {
    let mut lock = state.stdout.lock();
    lock.write(b"\x1b[?1049h\x1b[H\x1b[2C")
        .wrap_err("Could not write to stdout")?;

    lock.flush().wrap_err("Failed to flush stdout")?;

    Ok(())
}

fn draw_ui(state: &mut State) -> color_eyre::Result<()> {
    let mut lock = state.stdout.lock();
    lock.write(b"\x1b7\x1b[2J\x1b[H")
        .wrap_err("Could not write to stdout")?;

    for _ in 0..state.displayed_lines {
        lock.write(b"~ \x1b[1B\x1b[2D")
            .wrap_err("Could not write to stdout")?;
    }

    lock.write(b"\x1b8").wrap_err("Could not write to stdout")?;

    lock.flush().wrap_err("Failed to flush stdout")?;

    Ok(())
}

fn main() -> color_eyre::Result<()> {
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    cvt(unsafe { libc::tcgetattr(STDIN_FILENO, &raw mut termios) })
        .wrap_err("Could not get terminal parameters")?;

    let mut state = State {
        previous_io_settings: termios,
        current_io_settings: termios,
        stdout: std::io::stdout(),
        displayed_lines: 25,
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
        draw_ui(&mut state).wrap_err("Failed to draw UI")?;

        stdin_lock
            .read_exact(&mut buffer)
            .wrap_err("Could not read character from standard input")?;
        let c = buffer[0];
        if c == b'v' {
            println!("received v input");
        } else if c == b'q' {
            break;
        }
    }

    Ok(())
}
