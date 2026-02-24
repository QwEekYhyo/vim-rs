use crate::State;

#[derive(Debug)]
pub enum Command {
    Save { filename: Option<String> },
    Quit { forcefully: bool },
    SaveAndQuit { filename: Option<String> },
    None,
}

#[derive(Debug)]
pub enum ParseError {
    UnknownCommand(String),
    TrailingCharacters(String),
}

impl Command {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["q"] => Ok(Command::Quit { forcefully: false }),
            ["q!"] => Ok(Command::Quit { forcefully: true }),
            ["q" | "q!", trailing @ ..] => Err(ParseError::TrailingCharacters(trailing.join(" "))),
            ["w"] => Ok(Command::Save { filename: None }),
            ["w", filename @ ..] => Ok(Command::Save {
                filename: Some(filename.join(" ")),
            }),
            ["wq" | "x"] => Ok(Command::SaveAndQuit { filename: None }),
            ["wq" | "x", filename @ ..] => Ok(Command::SaveAndQuit {
                filename: Some(filename.join(" ")),
            }),
            [unknown, ..] => Err(ParseError::UnknownCommand((*unknown).to_owned())),
            [] => Ok(Command::None),
        }
    }
}

impl State {
    /// Returns true if the program should continue
    pub fn handle_command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Save { filename } => todo!(),
            Command::Quit { forcefully } => {
                if !forcefully && self.dirty {
                    "No write since last change (add ! to override)"
                        .clone_into(&mut self.message.msg);
                    return true;
                }
                return false;
            }
            Command::SaveAndQuit { filename } => todo!(),
            Command::None => todo!(),
        }

        true
    }

    pub fn handle_parse_error(&mut self, err: ParseError) {
        match err {
            ParseError::UnknownCommand(unknown) => {
                self.message = crate::Message {
                    msg: format!("Not an editor command: {}", unknown),
                    r#type: crate::MessageType::Error,
                }
            }
            ParseError::TrailingCharacters(trailing) => {
                self.message = crate::Message {
                    msg: format!("Trailing characters: {}", trailing),
                    r#type: crate::MessageType::Error,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::command_parser::{Command, ParseError};

    #[test]
    fn parse_q() {
        let cmd = Command::parse("q").unwrap();
        assert!(matches!(cmd, Command::Quit { forcefully: false }));

        let cmd = Command::parse("q!").unwrap();
        assert!(matches!(cmd, Command::Quit { forcefully: true }));

        let res = Command::parse("q trailing");
        assert!(matches!(res, Err(ParseError::TrailingCharacters(_))));

        let res = Command::parse("q! trailing");
        assert!(matches!(res, Err(ParseError::TrailingCharacters(_))));
    }

    #[test]
    fn parse_save() {
        let cmd = Command::parse("w").unwrap();
        assert!(matches!(cmd, Command::Save { filename: None }));

        let cmd = Command::parse("w file").unwrap();
        assert!(matches!(cmd, Command::Save { filename: Some(path) } if path == "file" ));

        let cmd = Command::parse("w very weird filename").unwrap();
        assert!(
            matches!(cmd, Command::Save { filename: Some(path) } if path == "very weird filename" )
        );

        let cmd = Command::parse("wq").unwrap();
        assert!(matches!(cmd, Command::SaveAndQuit { filename: None }));

        let cmd = Command::parse("wq file").unwrap();
        assert!(matches!(cmd, Command::SaveAndQuit { filename: Some(path) } if path == "file" ));

        let cmd = Command::parse("x").unwrap();
        assert!(matches!(cmd, Command::SaveAndQuit { filename: None }));

        let cmd = Command::parse("x file").unwrap();
        assert!(matches!(cmd, Command::SaveAndQuit { filename: Some(path) } if path == "file" ));
    }
}
