use color_eyre::eyre::Context;
use std::io::Read;
use std::os::fd::FromRawFd;

use cvt::cvt;
use libc::{ECHO, ICANON, ISIG, STDIN_FILENO, TCSAFLUSH, TCSANOW};

#[derive(Debug)]
struct State {
    previous_io_settings: libc::termios,
    current_io_settings: libc::termios,
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(STDIN_FILENO, TCSANOW, &raw const self.previous_io_settings);
        }
    }
}

fn main() -> color_eyre::Result<()> {
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    cvt(unsafe { libc::tcgetattr(STDIN_FILENO, &raw mut termios) })
        .wrap_err("Could not get terminal parameters")?;

    let mut state = State {
        previous_io_settings: termios,
        current_io_settings: termios,
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

    let mut stdin = unsafe { std::fs::File::from_raw_fd(STDIN_FILENO) };
    let mut buffer = [0u8; 1];

    loop {
        stdin
            .read_exact(&mut buffer)
            .wrap_err("Could not read character from standard input")?;
        let c = buffer[0];
        if c == b'v' {
            println!("received v input");
        } else if c == b'q' {
            break;
        }
    }
    // Do not close stdin or else bad things will happen
    // eg. next ioctl will fail and we won't be able to restore termios
    std::mem::forget(stdin);

    Ok(())
}
