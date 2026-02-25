use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

use crate::line::Line;

pub fn save_to_file<P: AsRef<Path>>(path: P, lines: &[Line]) -> std::io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    let mut writer = BufWriter::new(file);
    for line in lines {
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}
