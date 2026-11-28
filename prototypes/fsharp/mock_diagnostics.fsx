module MockDiagnostics

open MockModel

let renderProblem gateWord = function
    | UnclosedQuote quote -> sprintf "the quote `%c` is never closed" quote
    | UnexpectedClosing closing -> sprintf "unexpected closing bracket `%c`" closing
    | EmptyListItem -> "a comma-separated list has an empty item"
    | NestingLimit -> "syntax may nest at most 128 levels deep"
    | UnexpectedText text -> sprintf "unexpected text after the expression: `%s`" text
    | UnclosedBracket -> "a bracket opened here is never closed"
    | InlineComment -> "a comment goes on a line of its own"
    | MissingBindingKeyword text -> sprintf "a binding starts with `product`: write `product %s`" text
    | UnknownKeyword text -> sprintf "`%s` does not start with a keyword" text
    | StageNeedsName -> "`stage` needs a name: write `stage <name>:`"
    | InlineScriptWithBody (head, line) ->
        sprintf "`%s: %s` has lines under it: a script of several lines starts on the line after `%s:`" head line head
    | MissingScript (word, head) -> sprintf "`%s %s` needs a command: write `%s %s: <command>`" word head word head
    | UnexpectedIndent text -> sprintf "lines are indented under `%s`, which neither ends with `:` nor leaves a bracket open" text
    | ValueCount (head, count) -> sprintf "`%s` takes one value, not %d" head count
    | MissingName owner -> sprintf "%s needs a name" owner
    | DefaultUsedAsName -> "`default` is a prefix, not a name: write `default <keyword>: <value>`"
    | ReservedName name -> sprintf "`%s` is a reserved word, so it can't be a name" name
    | InvalidName name -> sprintf "`%s` is not a name: a name is letters, digits and `_`, and starts with a letter or `_`" name
    | NamedDefault head -> sprintf "a default applies to everything, so it names nothing: drop `%s`" head
    | DefaultTarget (word, value) -> sprintf "`default` is a prefix, not a name: write `default %s: %s`" word value
    | MissingSeparator (separator, text) -> sprintf "expected `%s` in `%s`" separator text
    | MissingDelimiters (opening, closing, text) -> sprintf "expected `%c ... %c` in `%s`" opening closing text
    | FolderExtension -> "a folder has no extension: drop the extension, or the `/`"
    | DescriptionOrder token ->
        sprintf "`%s` is out of place or repeated: a description is written `Type .ext / [dims] @gates`, each part at most once and in that order" token
    | EmptyGateAnnotation kind -> sprintf "`@%s` needs at least one gate" (gateWord kind)
    | RepeatedGateAnnotation kind ->
        let word = gateWord kind
        sprintf "`@%s` is written once, holding every %s, as in `@%s(a, b)`" word word word
    | ContradictedGate name -> sprintf "`%s` is both applied and turned off here" name
    | FolderSource name -> sprintf "`%s`: a source is a file, so it has no `/`" name
    | OutputPlacement -> "an output takes its dimensions from its call, so `[...]` and `here` are for sources"
    | InvalidMinimum text -> sprintf "`@min` needs a 32-bit integer, found `%s`" text
    | InvalidParameterAnnotation text -> sprintf "`%s` is not a parameter annotation" text
    | InvalidArgumentAnnotation text -> sprintf "`%s` is not an argument annotation" text
    | InvalidArgument text -> sprintf "cannot read the argument `%s`" text
    | BindingDescription text -> sprintf "a binding declares only its type and dimensions, not `%s`" text
    | ExpectedValueOnly (word, rest) -> sprintf "`%s` takes a value and no name: write `%s: %s`" word word ((rest.TrimStart ':').Trim())
    | ExpectedNameOnly (word, value) -> sprintf "`%s` takes a name and no value: drop `: %s`" word value
    | ExpectedName word -> sprintf "`%s` needs a name: write `%s <name>`" word word
    | MissingImportSource text -> sprintf "write `use: path` or `use alias: path [names]`, not `use %s`" text
    | InvalidImport text -> sprintf "write `use [alias:] path [names]`, not `%s`" text
    | TypeParameterNeedsVariable typ -> sprintf "a type's parameters are variables, written `$Name`, not `%A`" typ
    | InvalidTypeName name -> sprintf "`%s` is not a type name: a type's name is a name that starts with a capital letter" name
    | DefinedTypeVariable name -> sprintf "`$%s` is a variable, so it cannot be defined" name
    | NegatedDefaultGate (kind, name) ->
        let word = gateWord kind
        sprintf "a default applies a %s to everything, so it can't turn one off: write `@%s(!%s)` on what should skip it" word word name
    | GateNeedsScript (kind, name, head) ->
        let word = gateWord kind
        sprintf "the %s `%s` needs a command: write `%s %s: <command>`" word name word head
    | ReviewParameters name -> sprintf "the review `%s` takes no parameters: a person gives its verdicts" name
    | ReviewScript (name, value) ->
        sprintf "the review `%s` has no command, since a person gives its verdicts: drop `: %s`" name value
    | DisallowedDefault keyword -> sprintf "`%A` defines something, so it cannot have a default" keyword
    | ExpectedStageBody -> "write `stage <name>:` and indent what it holds"
    | UnexpectedBodyValue (head, value) ->
        sprintf "`%s` holds indented lines, so its line ends with `:` (found `%s` after it)" head value
    | NestedDefinition -> "definitions belong at the top of the file, not in a body"

let diagnosticLine diagnostic =
    1 + (diagnostic.Span.Source |> Seq.take diagnostic.Span.Start |> Seq.filter ((=) '\n') |> Seq.length)
