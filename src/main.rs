use color_eyre::eyre::Context;
use std::io::{Read, Write};

use cvt::cvt;
use libc::{ECHO, ICANON, ISIG, STDIN_FILENO, TCSAFLUSH, TCSANOW};

#[derive(Debug)]
struct State {
    previous_io_settings: libc::termios,
    current_io_settings: libc::termios,
    stdout: std::io::Stdout,
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

fn draw_ui(state: &mut State) -> color_eyre::Result<()> {
    let mut lock = state.stdout.lock();
    lock.write(b"\x1b[?1049h\x1b[2J\x1b[HWelcome to Vim-rs")
        .wrap_err("Could not write to stdout")?;

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

    draw_ui(&mut state)?;

    let mut lock = std::io::stdin().lock();
    loop {
        lock.read_exact(&mut buffer)
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
