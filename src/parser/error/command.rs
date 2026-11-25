use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandError {
    VerifyHoldsOneCommand {
        operation: String,
    },
    EmptyCommandTemplate,
    CommandLineIndentation {
        operation: String,
    },
    ExpectedCommandForm,
    ExpectedVerifyForm,
    RedirectNotAllowed {
        sign: String,
        what: String,
        stream: String,
        port: String,
        example: String,
    },
    SecondRedirect {
        what: String,
        sign: String,
        stream: String,
    },
    RedirectNeedsTarget {
        sign: String,
        what: String,
        port: String,
        stream: String,
        example: String,
    },
    RedirectTargetNotPlaceholder {
        sign: String,
        target: String,
        what: String,
        stream: String,
        port: String,
        example: String,
    },
    StdoutWrittenTwice {
        what: String,
        line: usize,
    },
    OperatorNotRedirection {
        operator: String,
        what: String,
    },
    OperatorNotPipe {
        operator: String,
        what: String,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VerifyHoldsOneCommand { operation } => write!(f, "a `verify` line holds one command, written after its `:`; give each command a `verify {operation}: ...` line of its own"),
            Self::EmptyCommandTemplate => f.write_str("command template must not be empty"),
            Self::CommandLineIndentation { operation } => write!(f, "indent each line of the command for `{operation}` as its first, or further to go on with the line above"),
            Self::ExpectedCommandForm => f.write_str("expected command: operation: executable [arguments]"),
            Self::ExpectedVerifyForm => f.write_str("expected verify operation: executable [arguments]"),
            Self::RedirectNotAllowed { sign, what, stream, port, example } => write!(f, "`{sign}` in {what} is not read as a redirection: only an operation's `command` connects its {stream} to an {port}, as in `{example}`; quote it ('{sign}') to pass it to the program"),
            Self::SecondRedirect { what, sign, stream } => write!(f, "{what} has a second `{sign}`; a command has one {stream}"),
            Self::RedirectNeedsTarget { sign, what, port, stream, example } => write!(f, "`{sign}` in {what} needs the {port} its {stream} goes to after it, as in `{example}`"),
            Self::RedirectTargetNotPlaceholder { sign, target, what, stream, port, example } => write!(f, "`{sign} {target}` in {what}: a command's {stream} is connected to one of its operation's {port}s, so write the {port}'s placeholder alone after `{sign}`, as in `{example}`"),
            Self::StdoutWrittenTwice { what, line } => write!(f, "{what} writes its stdout to an output on its line {line} already; only one of a command's lines writes an output as its stdout"),
            Self::OperatorNotRedirection { operator, what } => write!(f, "`{operator}` in {what} is not read as a redirection, since commands run without a shell; quote it ('{operator}') to pass it to the program. A command's stdout is written to an output with `> {{@output}}`, and its stdin read from an input with `< {{input}}`, each sign a word of its own"),
            Self::OperatorNotPipe { operator, what } => write!(f, "`{operator}` in {what} is not read as a pipe or redirection, since commands run without a shell; quote it ('{operator}') to pass it to the program, or run a shell with the paths as its arguments, as in sh -c 'tool \"$1\" | sort > \"$2\"' sh {{input}} {{@output}}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            CommandError::VerifyHoldsOneCommand {
                operation: "operation".to_owned(),
            },
            CommandError::EmptyCommandTemplate,
            CommandError::CommandLineIndentation {
                operation: "operation".to_owned(),
            },
            CommandError::ExpectedCommandForm,
            CommandError::ExpectedVerifyForm,
            CommandError::RedirectNotAllowed {
                sign: "sign".to_owned(),
                what: "what".to_owned(),
                stream: "stream".to_owned(),
                port: "port".to_owned(),
                example: "example".to_owned(),
            },
            CommandError::SecondRedirect {
                what: "what".to_owned(),
                sign: "sign".to_owned(),
                stream: "stream".to_owned(),
            },
            CommandError::RedirectNeedsTarget {
                sign: "sign".to_owned(),
                what: "what".to_owned(),
                port: "port".to_owned(),
                stream: "stream".to_owned(),
                example: "example".to_owned(),
            },
            CommandError::RedirectTargetNotPlaceholder {
                sign: "sign".to_owned(),
                target: "target".to_owned(),
                what: "what".to_owned(),
                stream: "stream".to_owned(),
                port: "port".to_owned(),
                example: "example".to_owned(),
            },
            CommandError::StdoutWrittenTwice {
                what: "what".to_owned(),
                line: 1,
            },
            CommandError::OperatorNotRedirection {
                operator: "operator".to_owned(),
                what: "what".to_owned(),
            },
            CommandError::OperatorNotPipe {
                operator: "operator".to_owned(),
                what: "what".to_owned(),
            },
        ];
        for variant in variants {
            let text = variant.to_string();
            assert!(!text.is_empty(), "{variant:?} has no wording");
            for held in held_text(&format!("{variant:?}")) {
                assert!(
                    text.contains(held),
                    "{variant:?} words {text:?} without `{held}`"
                );
            }
        }
    }

    fn held_text(debug: &str) -> Vec<&str> {
        debug.split('"').skip(1).step_by(2).collect()
    }
}
