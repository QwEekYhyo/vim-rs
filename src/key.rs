use std::{
    error::Error,
    fmt::Display,
    io::{Read, StdinLock},
};

#[derive(Debug)]
pub enum Key {
    Char(u8),
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Delete,
    Backspace,
    Enter,
    Tab,
}

#[derive(Debug)]
pub enum SequenceParsingError {
    UnknownSequence(Vec<u8>),
    UnknownChar(u8),
    NoChar,
}

impl Error for SequenceParsingError {}

impl Display for SequenceParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SequenceParsingError::UnknownSequence(seq) => {
                write!(f, "Received unknown escape sequence: {seq:?}")
            }
            SequenceParsingError::UnknownChar(c) => {
                write!(f, "Received unknown character: {}", *c as char)
            }
            SequenceParsingError::NoChar => write!(f, "Received no character",),
        }
    }
}

pub fn read_key(stdin: &mut StdinLock) -> Result<Key, SequenceParsingError> {
    let mut buf = [0u8; 1];
    if stdin.read(&mut buf).is_err() || buf[0] == 0 {
        return Err(SequenceParsingError::NoChar);
    }

    match buf[0] {
        b'\x08' | b'\x7f' => Ok(Key::Backspace),
        b'\n' | b'\r' => Ok(Key::Enter),
        b'\t' => Ok(Key::Tab),
        b'\x1b' => {
            // Read up to 7 more bytes non-blocking to consume the full sequence
            let mut seq = [0u8; 7];
            let n = match stdin.read(&mut seq) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(Key::Escape),
                Err(_) => Err(SequenceParsingError::NoChar)?,
            };

            if n == 0 {
                return Ok(Key::Escape);
            }

            Ok(parse_escape_sequence(&seq[..n])?)
        }
        _ => {
            if buf[0].is_ascii() || buf[0] == b' ' {
                Ok(Key::Char(buf[0]))
            } else {
                Err(SequenceParsingError::UnknownChar(buf[0]))?
            }
        }
    }
}

fn parse_escape_sequence(sequence: &[u8]) -> Result<Key, SequenceParsingError> {
    if sequence[0] == b'[' {
        return match &sequence[1..] {
            b"3~" => Ok(Key::Delete),
            b"A" => Ok(Key::ArrowUp),
            b"B" => Ok(Key::ArrowDown),
            b"C" => Ok(Key::ArrowRight),
            b"D" => Ok(Key::ArrowLeft),
            _ => Err(SequenceParsingError::UnknownSequence(sequence.to_owned())),
        };
    }

    Ok(Key::Escape)
}
