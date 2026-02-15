use color_eyre::eyre::Context;
use cvt::cvt;
use libc::STDIN_FILENO;

fn main() -> color_eyre::Result<()> {
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    cvt(unsafe { libc::tcgetattr(STDIN_FILENO, &raw mut termios) })
        .wrap_err("Could not get terminal parameters")?;

    println!("{termios:?}");

    println!("Hello, world!");

    Ok(())
}
