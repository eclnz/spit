//! The command line: its commands and flags, how they are read, and the
//! help that describes them.

use std::fmt;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Command {
    Check,
    Inputs,
    Dag,
    Artifacts,
}

const COMMANDS: [Command; 4] = [
    Command::Check,
    Command::Inputs,
    Command::Dag,
    Command::Artifacts,
];

struct CommandSpec {
    pub(crate) name: &'static str,
    files: &'static str,
    summary: &'static str,
    example: &'static str,
    flags: &'static [Flag],
}

impl Command {
    fn spec(self) -> CommandSpec {
        use Flag::{
            Commands, Hovers, Json, Output, Partial, PathRules, Paths, Root, Stdin, Unmatched,
        };
        match self {
            Self::Check => CommandSpec {
                name: "check",
                files: "<pipeline.spit | recipe.spitin | inputs.spitout>",
                summary: "step 1: compile a pipeline, check a recipe against its pipeline, or check a .spitout's syntax; reads no data",
                example: "spit check analysis.spit\n  spit check dataset.spitin",
                flags: &[PathRules, Json, Stdin, Hovers],
            },
            Self::Inputs => CommandSpec {
                name: "inputs",
                files: "<recipe.spitin>",
                summary: "step 2: find a dataset's sources with a recipe, apply `exclude`, `drop` and `require`, and write a .spitout",
                example: "spit inputs dataset.spitin -o dataset.spitout",
                flags: &[Root, Output, Unmatched],
            },
            Self::Dag => CommandSpec {
                name: "dag",
                files: "<recipe.spitin> or <pipeline.spit> <inputs.spitout | ->",
                summary: "step 3: resolve a pipeline's jobs over a dataset's inputs; -o writes the .spitdag",
                example: "spit dag dataset.spitin -o analysis.spitdag\n  spit dag analysis.spit dataset.spitout -o analysis.spitdag\n  spit dag dataset.spitin --commands",
                flags: &[Root, Paths, Commands, Partial, Json, Output],
            },
            Self::Artifacts => CommandSpec {
                name: "artifacts",
                files: "<recipe.spitin> or <pipeline.spit> <inputs.spitout | ->",
                summary: "step 3: report what can and cannot be made from a dataset's inputs, and why",
                example: "spit artifacts dataset.spitin\n  spit artifacts analysis.spit dataset.spitout",
                flags: &[Root],
            },
        }
    }

    pub(crate) fn name(self) -> &'static str {
        self.spec().name
    }

    fn parse(name: &str) -> Option<Self> {
        COMMANDS.into_iter().find(|command| command.name() == name)
    }

    /// The files it takes, as the usage line shows them.
    fn files(self) -> &'static str {
        self.spec().files
    }

    /// The most files it takes; every command takes at least one.
    fn most_files(self) -> usize {
        match self {
            Self::Check | Self::Inputs => 1,
            Self::Dag | Self::Artifacts => 2,
        }
    }

    fn summary(self) -> &'static str {
        self.spec().summary
    }

    fn example(self) -> &'static str {
        self.spec().example
    }

    /// Given a recipe in place of a `.spitout`, or earlier files in place of
    /// a `.spitdag`, the command runs the steps between in memory.
    fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Dag | Self::Artifacts => Some(
                "Given a .spitin in place of the .spitout, it runs `spit inputs` in memory first.\nA .spitin names its own pipeline, so it is given alone; a .spitout or `-` needs the pipeline first.",
            ),
            Self::Check | Self::Inputs => None,
        }
    }

    /// The flags this command accepts.
    fn flags(self) -> &'static [Flag] {
        self.spec().flags
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Flag {
    Root,
    Output,
    Paths,
    Commands,
    Partial,
    Unmatched,
    PathRules,
    Json,
    Stdin,
    Hovers,
}

const FLAGS: [Flag; 10] = [
    Flag::Root,
    Flag::Output,
    Flag::Paths,
    Flag::Commands,
    Flag::Partial,
    Flag::Unmatched,
    Flag::PathRules,
    Flag::Json,
    Flag::Stdin,
    Flag::Hovers,
];

/// Pairs of flags that cannot be used together.
const CONFLICTS: [(Flag, Flag); 7] = [
    (Flag::Json, Flag::Paths),
    (Flag::Json, Flag::Output),
    (Flag::Paths, Flag::Output),
    (Flag::Json, Flag::Commands),
    (Flag::Commands, Flag::Output),
    (Flag::Json, Flag::PathRules),
    (Flag::Unmatched, Flag::Output),
];

impl Flag {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Root => "--root",
            Self::Output => "-o",
            Self::Paths => "--paths",
            Self::Commands => "--commands",
            Self::Partial => "--partial",
            Self::Unmatched => "--unmatched",
            Self::PathRules => "--path-rules",
            Self::Json => "--json",
            Self::Stdin => "--stdin",
            Self::Hovers => "--hovers",
        }
    }

    /// What the flag's value is, for a flag that takes one.
    pub(crate) fn value(self) -> Option<&'static str> {
        match self {
            Self::Root => Some("<directory>"),
            Self::Output => Some("<file>"),
            _ => None,
        }
    }

    fn help(self, command: Command) -> &'static str {
        match (self, command) {
            (Self::Root, _) => {
                "with a .spit pipeline and no recipe, the dataset folder to scan, relative to where spit runs"
            }
            (Self::Output, Command::Inputs) => "write the .spitout to <file>, not standard output",
            (Self::Output, _) => "write the .spitdag to <file>",
            (Self::Paths, _) => "show each artifact's file",
            (Self::Commands, _) => {
                "show each job's command lines, as a shell would run them; cannot combine with -o"
            }
            (Self::Partial, _) => "plan complete jobs and record artifacts that cannot be produced",
            (Self::Unmatched, _) => {
                "list files matching no source rule instead of writing a .spitout"
            }
            (Self::PathRules, _) => "list the path rule each product uses",
            (Self::Json, Command::Check) => "print diagnostics as JSON, for editors",
            (Self::Json, _) => "print the .spitdag",
            (Self::Stdin, _) => {
                "read the file's text from standard input; the file names its location"
            }
            (Self::Hovers, _) => {
                "include hovers with --json: SPIT's own words, and a pipeline's operations and products"
            }
        }
    }

    fn parse(name: &str) -> Option<Self> {
        if name == "--output" {
            return Some(Self::Output);
        }
        FLAGS.into_iter().find(|flag| flag.name() == name)
    }
}

/// The flags given on the command line, and each one's value.
#[derive(Default)]
struct Flags(Vec<(Flag, Option<String>)>);

impl Flags {
    /// Add the flag `argument` for `command`, taking its value from `rest`
    /// when it has one.
    fn add(
        &mut self,
        command: Command,
        argument: &str,
        rest: &mut impl Iterator<Item = String>,
    ) -> Result<(), String> {
        let flag = Flag::parse(argument)
            .ok_or_else(|| misuse(format_args!("unknown option `{argument}`"), Some(command)))?;
        if !command.flags().contains(&flag) {
            let accepting: Vec<_> = COMMANDS
                .iter()
                .filter(|other| other.flags().contains(&flag))
                .map(|other| other.name())
                .collect();
            return Err(misuse(
                format_args!("{} applies to {}", flag.name(), accepting.join(", ")),
                Some(command),
            ));
        }
        if self.has(flag) {
            return Err(misuse(
                format_args!("{} is given more than once", flag.name()),
                Some(command),
            ));
        }
        let value = match flag.value() {
            Some(value) => Some(rest.next().ok_or_else(|| {
                misuse(
                    format_args!("{} needs a value: {value}", flag.name()),
                    Some(command),
                )
            })?),
            None => None,
        };
        self.0.push((flag, value));
        Ok(())
    }

    /// Fail if two flags that cannot be used together were both given.
    fn check_conflicts(&self, command: Command) -> Result<(), String> {
        if self.has(Flag::Hovers) && !self.has(Flag::Json) {
            return Err(misuse("--hovers requires --json", Some(command)));
        }
        for (first, second) in CONFLICTS {
            if self.has(first) && self.has(second) {
                if (first, second) == (Flag::Commands, Flag::Output) {
                    return Err(misuse(
                        format_args!("--commands cannot be used with -o; run dag with --commands to inspect command lines, or with -o <file> to save a .spitdag"),
                        Some(command),
                    ));
                }
                return Err(misuse(
                    format_args!("{} cannot be used with {}", first.name(), second.name()),
                    Some(command),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn has(&self, flag: Flag) -> bool {
        self.0.iter().any(|(given, _)| *given == flag)
    }

    pub(crate) fn value(&self, flag: Flag) -> Option<String> {
        self.0
            .iter()
            .find(|(given, _)| *given == flag)
            .and_then(|(_, value)| value.clone())
    }
}

pub(crate) struct CliArgs {
    pub(crate) command: Command,
    /// The file every command takes.
    pub(crate) file: String,
    /// The inputs after a pipeline, for a command that takes two files.
    pub(crate) second: Option<String>,
    flags: Flags,
}

impl CliArgs {
    pub(crate) fn has(&self, flag: Flag) -> bool {
        self.flags.has(flag)
    }

    pub(crate) fn value(&self, flag: Flag) -> Option<String> {
        self.flags.value(flag)
    }
}

/// What the command line asks for.
pub(crate) enum Request {
    Run(CliArgs),
    Help(Option<Command>),
    Version,
}

/// `spit help`, or `spit help <command>`.
pub(crate) struct Help(pub(crate) Option<Command>);

impl fmt::Display for Help {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => overview(f),
            Some(command) => command_help(f, command),
        }
    }
}

fn overview(f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(
        "spit: compile a pipeline, settle a dataset's inputs, and resolve its jobs into a .spitdag\n\nusage: spit <command> <files> [options]\n\ncommands:\n",
    )?;
    for command in COMMANDS {
        writeln!(f, "  {:<10} {}", command.name(), command.summary())?;
    }
    f.write_str(
        "\nfiles:\n  .spit      a pipeline: sources, operations, steps, commands, path rules\n  .spitin    a recipe for a dataset's inputs, naming its pipeline\n  .spitout   a dataset's settled inputs, each source with its file\n  .spitdag   the resolved jobs, each with its files and command\n\nRun `spit help <command>` for its options.\n",
    )
}

fn command_help(f: &mut fmt::Formatter<'_>, command: Command) -> fmt::Result {
    let name = command.name();
    writeln!(f, "spit {name}: {}\n", command.summary())?;
    let options = if command.flags().is_empty() {
        ""
    } else {
        " [options]"
    };
    writeln!(f, "usage: spit {name} {}{options}", command.files())?;
    if let Some(shortcut) = command.shortcut() {
        writeln!(f, "\n{shortcut}")?;
    }
    if !command.flags().is_empty() {
        writeln!(f, "\noptions:")?;
        for flag in command.flags() {
            let name = match flag.value() {
                Some(value) => format!("{} {value}", flag.name()),
                None => flag.name().to_owned(),
            };
            writeln!(f, "  {name:<20} {}", flag.help(command))?;
        }
    }
    writeln!(f, "\nexample:\n  {}", command.example())
}

/// A usage error: what is wrong, and where to read more.
fn misuse(problem: impl std::fmt::Display, command: Option<Command>) -> String {
    let more = command.map_or_else(
        || "run `spit help`".to_owned(),
        |command| {
            format!(
                "usage: spit {} {} [options]; run `spit help {}`",
                command.name(),
                command.files(),
                command.name()
            )
        },
    );
    format!("{problem}\n{more}")
}

pub(crate) fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Request, String> {
    let mut args = args.into_iter();
    let command = match args.next().as_deref() {
        None | Some("help" | "--help" | "-h") => {
            return match args.next() {
                None => Ok(Request::Help(None)),
                Some(name) => Command::parse(&name)
                    .map(|command| Request::Help(Some(command)))
                    .ok_or_else(|| misuse(format_args!("unknown command `{name}`"), None)),
            };
        }
        Some("--version" | "-V") => return Ok(Request::Version),
        Some(name) => Command::parse(name)
            .ok_or_else(|| misuse(format_args!("unknown command `{name}`"), None))?,
    };
    let mut files = Vec::new();
    let mut flags = Flags::default();
    while let Some(argument) = args.next() {
        if matches!(argument.as_str(), "--help" | "-h") {
            return Ok(Request::Help(Some(command)));
        }
        if argument == "-" || !argument.starts_with('-') {
            files.push(argument);
            continue;
        }
        flags.add(command, &argument, &mut args)?;
    }
    flags.check_conflicts(command)?;
    let (file, second) = take_files(command, files)?;
    Ok(Request::Run(CliArgs {
        command,
        file,
        second,
        flags,
    }))
}

/// The file `command` takes, and a second when it takes two.
fn take_files(command: Command, files: Vec<String>) -> Result<(String, Option<String>), String> {
    let mut files = files.into_iter();
    let Some(file) = files.next() else {
        return Err(misuse(
            format_args!("{} needs {}", command.name(), command.files()),
            Some(command),
        ));
    };
    let second = files.next();
    let extra = if command.most_files() == 1 {
        second.as_ref()
    } else {
        files.as_slice().first()
    };
    if let Some(extra) = extra {
        return Err(misuse(
            format_args!("unexpected file `{extra}`"),
            Some(command),
        ));
    }
    Ok((file, second))
}
