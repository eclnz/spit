# Mock language plan

The OCaml prototype runs with `./ocaml/parser.sh`, which parses all five mocks and prints one JSON result per file. Pass file paths to parse other inputs, or `--stdin` to read standard input. `./ocaml/parser.sh --test` compiles with warnings treated as errors and runs the regression suite. The launcher requires `ocamlc`, builds in a temporary directory and removes its build products. A parse diagnostic gives a nonzero CLI exit while retaining valid declarations in the JSON result.

The compiler experiment runs with `./ocaml/spit.sh check pipeline.spit` and `./ocaml/spit.sh dag pipeline.spit inventory.spitout`. The latter writes a version-18 DAG to standard output. A flat inventory declares `root data` before `sources:` for execution; its root is relative to the inventory file. An inventory without a root produces a DAG with a null dataset root. Pipeline `output:` and inventory `output` select the output root. `./ocaml/spit.sh --test` checks the compiler and executes a weather fixture through the existing runner, found at `.venv/bin/spit-bash` or the path in `SPIT_BASH`.

OCaml sources use ocamlformat 0.30.0 with conventional formatting, a 100-column margin and blank lines between definitions. Run `./ocaml/format.sh`, or `./ocaml/format.sh --check`; `SPIT_OCAMLFORMAT` can name the formatter binary. Physical line counts include that readable layout.

The mock runs with `dotnet fsi prototypes/fsharp/mock_parser.fsx` over `v1_mock.spit` to `v4_mock.spit` and `stats_mock.spit`. `mock_model.fsx` holds the model and keyword table. `mock_diagnostics.fsx` holds problems and their rendering. It redesigns SPIT's syntax as a few consistent rules, written out in F#, so the data and each transformation are explicit. Nothing in `src/` changes, and the mock is judged on its own terms, not against the Rust parser.

This plan collects the findings of a review of the mock. Every finding was reproduced with a scratch input. The language's design is fixed first, then the parser's robustness. Mark each step done with what changed.

The existing language reference is the behavior contract for this syntax redesign. Stage visibility, dependency ordering, default inheritance and precedence, gate effects, iteration, imports and command expansion follow the documented behavior unless a change is explicitly agreed. Open discussions concern how to express that behavior. Parser limitations and internal data representations do not establish a need for new syntax or new semantics.

## Language design

### D1. Continuation is too loose

- A line indented too far becomes a continuation, so `product a = f(x)` followed by an over-indented `product b = g(a)` loses `b`, and `a` gets the argument `"x) product b = g(a"`. The same happens to `command c:` and `command d:`.
- Multi-line commands are joined with spaces (`continued`), so they lose their line breaks. `lines` also drops every line starting with `#`, even inside a quoted script. A `python3 -c '...'` block came back as broken Python with its comment deleted.
- Whether indented lines are a list or a continuation depends only on the first line. Under `command one:`, one quoted awk program spread over two lines became two commands.
- A signature written over two lines cannot have a body, because rule 3 joins the body into the signature, and the error names the wrong cause.
- The original v3 put a call's closing `)` on its own line. That now fails, so calls end in `))`: the language bent to fit the parser.
- **Fix:** a line continues only while a bracket is open. A command's body is kept as written, with its line breaks and its `#` lines.

### D2. Comments

- `#` partway through a line becomes data. `output ../out # where it goes` and `path reading: ... # note` keep the comment in the value.
- **Fix:** a comment is a whole line, at the level of entries. A `#` outside quotes elsewhere is an error that says so. Inside a command's body, a `#` belongs to the command.

### D3. A colon means different things on different keywords

- `root C:/data` gives the root `"C"`, because every line is split at its first colon, whether or not its keyword takes a name.
- **Fix:** one rule. Text before the colon is the name, and text after it is the value. A keyword with only a value writes it after a colon: `root: ../network`, `types: strict`, `dimensions: [region, site]`, `use: stats_mock.spit`.

### D4. Modifiers are an unordered bag, and the last one wins

- `.csv .txt [a] [b]` keeps `.txt` and `[b]`.
- `Catalogue from x /` becomes a plain `Folder`, so the `from` is lost.
- `X .csv / view .tsv` gives a view with `.tsv`.
- `beside here` reads `here` as the name of the file beside it.
- `@check(c) @check(!c)` keeps both.
- `Description` and `Output` allow a folder, `beside` and an extension in any combination.
- **Fix:** each modifier at most once, in a fixed order, with one sum type per shape: file, folder, folder copied from an input, view.

### D5. Empty names, and `default` in a name

- `check: nonempty` gives a gate named `""` whose command is `"nonempty"`, because `readGate` never calls `target`.
- A `default:` line under `path:` gives a path for an artifact called `default`.
- **Fix:** an empty name is an error everywhere, and `default` is reserved wherever a name may go.

### D6. Gate kinds are too alike

- `GateDef.Command` is optional for every kind, so `review looked_at: rm -rf /` parses, and so does `check orphan` with no command.
- A stage may write `default check: !nonempty` to remove a check inherited from a wider scope, as the current language reference specifies.
- **Fix:** the kinds share syntax but differ in the model. A check and a screen have a command; a review has none. Negation removes an inherited gate, including through a stage default.

### D7. Default eligibility and setting representations

The agreed language rule allows defaults for these settings:

- Operations: `verify`, `calculate`, `resources`, `uses`, `anchored`.
- Artifacts: `path`, `ext`, `check`, `screen`, `review`.
- Dimensions: `entities`.

Gate defaults apply existing gate definitions. Declarations (`source`, `operation`, `product`, `stage`, `type`, `tool`) and pipeline-wide settings (`types`, `root`, `output`, `dimensions`, `use`) have no default form. A `command` must target a named operation; `default command` is invalid. The eligibility set includes `default verify` and `default calculate`.

Implementation work remains:

- Apply the agreed eligibility set in the keyword table. Its current `stage` entry permits a default.
- Use `Ext` for extension settings, including the leading-dot validation used by output extensions.
- Give entity formatting and dimension renaming distinct typed representations.

### D8. Operation bodies

- An operation body accepts a `source`, a nested `stage` and a `command` for the operation itself, with no diagnostic.
- **Fix:** an operation body has its own scope type, holding only bindings and the operations they call. D1's bracket rule ends a signature at its closing bracket, so a multi-line signature can have a body.

### D9. Jobs have no identity

- `Made of call * output` copies the `Call` into each output, so two `product` lines with the same call cannot be told from one call with two outputs.
- The agreed language has no artifact aliases. Every `product` right-hand side is an explicit operation call, such as `cleaned(reading)` or `generate()`. A bare reference such as `product d = cleaned` is invalid. The parser still needs to enforce this distinction.
- Arguments are raw strings, not `Ref`s.
- **Fix:** a table of jobs, artifacts that refer to a job and an output, and arguments that are references.

### D10. Namespaces stop at types, and `.` means too much

The agreed type-reference spelling is `namespace.Type`, including generic forms such as `stats.Readings<$Kind>`. Namespace names may be lowercase, and type names start with a capital letter. This uses the qualification form already used by operations and gates. Import visibility follows the language reference. Placeholder path properties use `$property`, and input dimension projection uses `[dimension]`, inside the existing braces.

- `-> stats.Mean` fails with "not a modifier". v3 uses the library's `Mean`, `Script` and `Shift` unqualified, and its import list `[nonempty, average, what_if]` doesn't name them.
- `.` means a namespace, an extension (`.csv`), a field (`{report.dir}`) and a path.
- **Agreed syntax:** types use namespace qualification. Output path properties use `{report$dir}`, `{report$stem}` and `{report$prefix}`. Input dimension projection uses `{means[site]}`. The unnamed output uses the same property form, such as `{@output$dir}`. These spellings retain the reference's behavior, including collection order and declared-extension handling.

### D11. Settings that appear once take the first value silently

- `types strict` then `types standard` keeps `strict`, and two `root` lines keep the first (`tryPick` in `pipelineOf`).
- **Fix:** the keyword table marks what may appear once, and a second one is an error.

### D12. Smaller design points

- The reference gives a port name precedence over a same-named job dimension in a placeholder. Preserve that lookup rule in the prototype syntax.
- The stages `site` and `region` share their names with dimensions, and the stage `clean` with an operation, which makes `{@stage}` paths hard to read.
- `@` has three meanings: modifiers, built-ins such as `{@output}`, and command functions such as `@scalar`, `@join` and `@list`. The command functions are in neither rule 6 nor the parser.
- `TypesDecl.Library` holds `strict`, which is a mode, not a library.
- `readArg` allows one annotation, so `@vary(day) @where(site=dune)` is rejected.

## Parser robustness

### P1. Brackets are not matched

- `applied` takes the first `(` and the last `)`, and `topLevel` lets the depth go negative. `f(y) @check(z)` gives the argument `"y) @check(z"`, and `f(x)(y)` gives `"x)(y"`, with no diagnostic.
- **Fix:** match brackets, and make a negative depth, or text after the closing bracket, an error.

### P2. Names are not checked

- `-> A,B` gives a type `"A,B"`, `-> Mean.csv` a type `"Mean.csv"`, and `f(x many, y z)` parameters named `"x many"` and `"y z"`. `type readings` defines a lowercase type that `isType` can never refer to.
- **Fix:** names are identifiers, and types are identifiers that start with a capital letter.

### P3. Errors are exceptions

- `@min(two)` throws a `FormatException`, and `lowerEntry` catches only `Failure`, so the whole run stops.
- **Fix:** readers return `Result`, and every error is a diagnostic.

### P4. Leftover special cases

- `text.Contains " = "` guesses at a binding, and the message about a trailing colon is built by cutting strings.
- **Fix:** remove them once D1 and P3 are done, and see what still needs a hint.

## Structure

### R. Parser structure

- Keep keyword spelling, lexical form, default eligibility and scope in one table. `readStatement` owns the exhaustive semantic dispatch. `default` is a lexical prefix. D7 and D11 track additional default and singleton rules.
- Keep the original text in `SourceSpan.Source`, with start and exclusive end offsets. Lines, headers, entry values and script bodies refer to it. `readUntil` and `readDelimited` return consumed and remaining spans. Indentation delimits the body after the header.
- `readUntil` handles structural delimiters, quotes, comments and stop tokens. `readTypePrefix` owns generic arguments. `commaItems` consumes comma-separated values through a supplied reader. Syntax and scope nesting are capped at 128 levels.
- Represent entry content as `Bare`, `Value`, `Script` or `Body`. Each reader handles the cases it accepts.
- Compose the description readers in syntax order, then validate the gate annotations. `validateGates` returns the annotations with their spans. Declaration readers return annotated data. `lowerScope` and `lowerStatement` build the final model, and `gatesOn` creates settings during lowering.
- Readers return a `Diagnostic` containing both a `Span` and a `Problem`. Annotations retain their spans through validation. Render diagnostics at the output boundary, deriving the line from the source offset.
- The enclosing scope owns its physical extent. Each `entries` call establishes a read limit from its supplied lines, and `readEntry` respects that limit. Continuations inside a stage remain within the stage body.
- `validateContext` checks default eligibility and scope before reading a statement. Stage names are checked before their child bodies.
- `readStatement` dispatches to declaration readers. `ParsedScope.Entries` preserves statements and diagnostics in source order. Lowering preserves partial scopes and diagnostic order.
- `show` uses the structured formatter with zero-width layout. Literal whitespace is preserved. Loading the parser exposes its functions, and running it directly executes the CLI.
- Run `dotnet fsi --warnaserror mock_parser_tests.fsx` for parser regression checks.

### Placeholder behavior baseline

The agreed command interpolation syntax encloses every SPIT value expansion in braces. Built-in functions are written `{@scalar(offset)}`, `{@join(means)}` and `{@list(means)}`. Their documented behavior is retained. Operation-input annotations such as `@vary(day)` belong to pipeline statements.

The command reference supplies the existing behavior for the proposed selector spellings:

- Path properties `dir`, `stem` and `prefix` apply to outputs. `stem` uses the declared extension; a folder's stem is its whole name. The unnamed output has the same properties.
- Input dimension projection gives each artifact's dimension value. Collections produce one argument per artifact in the same order as their paths; a single input produces that artifact's value.
- Port names take precedence over same-named job dimensions in bare placeholders.
- `@join` produces one comma-separated argument, `@list` produces the path of a file listing the paths, and `@scalar` reads a numeric input.

These behaviors are documented in [command placeholders](../docs/guide/operations.md#placeholders). The agreed spellings are `{report$dir}`, `{report$stem}`, `{report$prefix}` and `{means[site]}`, with the same outer braces and the reference's existing behavior.

### Stage and default baseline

The current language reference supplies the intended baseline:

- Stages group work and establish defaults. Stages nest, and `{@stage}` is the stage path, such as `preprocess/combine`.
- Operation names are global. Products can be referenced across stages. Dependencies determine job order.
- Repeating a stage header at the same level continues the same stage.
- File defaults cover its stages. Stage defaults cover that stage and descendant stages.
- Paths, extensions and entity spelling use the nearest applicable default. A product-specific path has precedence over a default path.
- Check, review and tool lists accumulate from outer scopes inward. `!name` removes an inherited member.
- Each file or stage sets a particular default once.
- General default checks cover generated outputs. Their scope follows the step's call site. Extension-specific defaults can also cover sources.
- Tool defaults follow the operation's declaration site. Imported operations retain their own file's tool dependencies.

These rules are documented in [stages and defaults](../docs/guide/pipelines.md#stages), [default checks](../docs/guide/operations.md#default-checks) and [tools](../docs/guide/operations.md#tools). Applying declaration-site or call-site inheritance to the prototype's additional default categories still needs an explicit rule.

## Steps

The earlier combined checkpoint is preserved on `codex/ocaml-dag-prototype`. This branch isolates the OCaml compiler experiment with version-18 output. The earlier validation remains recorded in that branch. Replacement verification on this branch concerns the existing runner and format. The plan stays while the broader compiler conversion is being developed.

### Focused function refactor

- [x] Consolidate selector execution and decompose checking, resolution, loading and composite expansion into explicit data operations.
- [x] Separate lexer handlers, operation parsing/lowering, expression terms, selector interpretation and execution settings.
- [x] Separate command expansion/validation, inventory records, job serialization and graph summaries.
- [x] Organize parser tests into named groups; remeasure every function and inspect remaining long functions.
- [x] Run parser/compiler/runner checks, verify output bytes against the previous binaries, and compare scaling and total source lines. Preserved from the earlier prototype checkpoint.

All 94 CLI cases were executed through a wrapper comparing the saved and refactored binaries' stdout, stderr and exit status. Every case matched; runner integrations and parser regressions pass. The five mock parser JSON outputs also match. Five-run median bytecode DAG timings: 1,000 jobs 0.094→0.096 seconds, 4,000 jobs 0.394→0.402 seconds. No regression threshold was exceeded.

Production functions over 40 lines fell from 39 to 9. Representative inclusive lengths: checking 395→22, resolution 246→16, import loading 164→37, composite expansion 219→16, command scanning 154→28, command expansion 127→14. Remaining long functions include cohesive mappings/scanners, the keyword dispatcher and a checked-step coordinator with its output record. Parser tests have named groups instead of one 446-line initializer. Total production source is 5,539→5,743 lines (+204, 3.7%): explicit helper interfaces and stage-local data records increase the physical count while reducing individual function scope. Preserved from the earlier prototype checkpoint.

### Replacement verification before DAG changes

The OCaml experiment writes the existing version-18 DAG format. Its branch changes no runner code or DAG-format documentation. The artifact-table DAG design is separate follow-on work, held until the replacement is verified.

Replacement verification requires:

- Agreed syntax and source locations, without reading inventories during parsing.
- Behavioral comparisons with the existing compiler through equivalent pipeline fixtures and supplied inventories. Compare artifact identities, paths, types, commands, collections, resources, dependency edges and runtime results; retain explicit failures for unsupported behavior.
- Complete support for the reference behavior: gates, discovery and recipes, folder/view/sidecar binding, tool environments, partial plans, editor/CLI diagnostics and accurate provenance. Passing the current prototype cases is not full parity.
- Deterministic output, malformed-input and depth/scale checks, and runner execution/reuse checks against version 18.

The reference implementation is an executable oracle. Its source and compiler version are not inspected. Fixture spellings may differ where the agreed syntax deliberately differs. Generator and source-location metadata are compared separately from planned work; absent provenance remains a replacement gap, not something to normalize away and call complete.

Checkpoint `d0da9010` isolates this experiment from the artifact-table work on `codex/dag-artifact-table`. `python3 ocaml/tools/verify_replacement.py` passes a paired copy/aggregation fixture: version-18 planned work matches, inventory reordering preserves bytes, both compilers reject four invalid pipelines and a collection below its minimum, and the runner reuses all six jobs across compilers. Generator, source-file and job-origin metadata differ. The 94 compiler cases, parser suite, debug Cargo build/tests/clippy, format checks and strict documentation build pass. This bounded fixture does not certify replacement completeness.

Path canonicalization remains a gap: the reference resolves a temporary-folder symlink to its physical path, while OCaml retains the lexical path. The parity fixture supplies an already canonical root so it can compare execution paths strictly; it does not establish symlink parity. Missing features listed below and provenance require further verification before replacing the existing compiler.

`python3 ocaml/tools/verify_replacement.py` checks one paired copy/aggregation pipeline against the built reference executable and the existing runner. Its version-18 planned work matches, shuffled inventories produce identical bytes, four invalid pipelines and an undersized collection fail, and the runner executes the reference plan and reuses all six jobs from the OCaml plan. Generator, source-file and origin metadata still differ. The fixture uses canonical paths: symlink path canonicalization also remains a replacement gap. This bounded check does not certify replacement readiness.

### Parser algorithms and interface fixes

- [x] Preserve lexical command roles through binding, including escaped redirection and shell-operator arguments.
- [x] Parse located path/entity patterns once, select effective rules once per product and bind only values per artifact.
- [x] Replace quadratic gate/default membership checks and repeated diagnostic source scans.
- [x] Give scope construction a data-focused owner, remove parser/expansion adapter round trips, and separate compile-only pipeline metadata from syntax.
- [x] Consolidate source/binding grammar and script-line interfaces that duplicate conversions or fabricate entry context.
- [x] Run parser/compiler/runner regressions and before/after scaling checks, reporting code-size changes. Preserved from the earlier prototype checkpoint.

Verification: all 94 compiler CLI cases, runner integrations and parser checks pass. The parser suite includes escaped/quoted operators, continued redirects, located patterns, source-form equivalence, 10,000 gate uses and 10,000 extension defaults. All five mock syntax outputs and the 4,000-job DAG match the prior binaries byte-for-byte on supported inputs. Invalid pattern syntax is reported before inventory binding.

Bytecode CLI medians on this machine: 4,000 gate uses 0.535→0.035 seconds; 4,000 extension defaults 0.220→0.079 seconds; 4,000 errors with JSON locations 1.482→0.048 seconds; 4,000 reopened stages 0.105→0.105 seconds; 4,000-job DAG 0.446→0.403 seconds. Parser source is 494→453 lines and path binding 201→154 lines. Total production OCaml is 5,449→5,539 lines: typed patterns, source indexing and explicit scope support add 90 net lines. Preserved from the earlier prototype checkpoint.

### OCaml module organisation

- [x] Group syntax, compilation, planning, binding and output compilation units under `ocaml/src/`; name shared syntax modules without the prototype prefix. Preserved from the earlier prototype checkpoint.
- [x] Separate reusable `Json` encoding from `Syntax_json` inspection, and share `ocaml/build.sh` between the compiler and parser launchers. `Command_check` validates commands during compilation; `Command` binds them to planned artifacts. Preserved from the earlier prototype checkpoint.
- [x] Put command-line entry points in `ocaml/bin/`, tests in `ocaml/tests/` and pipeline examples in `ocaml/examples/`; retain the earlier prototype under `prototypes/fsharp/`. The formatter configuration belongs to `ocaml/`. Preserved from the earlier prototype checkpoint.
- [x] Check eight parser inputs and a 4,000-job DAG against the previous binaries: exit status, diagnostics and output bytes match. All 81 compiler CLI cases, runner integrations and parser checks pass. The 4,000-job DAG median over seven runs is 0.449 seconds before and 0.453 seconds after. Format checks pass; launchers work outside the checkout and build into temporary folders. Preserved from the earlier prototype checkpoint.

### Next OCaml implementation pass

- [x] Fix the remaining parser review findings: flag input loss, extension components, stage accumulation, retained source locations, command parsing ownership and duplicated header reading.
- [x] Implement imports and composite expansion using the agreed syntax and reference behavior, including selected imports, relative paths, cycles, private dependencies and diagnostics.
- [x] Run focused regression checks, parser scaling and compiler/runner integration. Preserved from the earlier prototype checkpoint. Further compiler features remain separate milestones in the OCaml conversion.

- [x] Add composed selectors, scoped resources, anchoring, scalar command reads and bounded calculation expressions. Runner integration covers scalar bounds and arithmetic precedence. Preserved from the earlier prototype checkpoint.

### OCaml compiler experiment

- [x] Build a checked representation with distinct IDs, dependency order, reference validation and representative type checking. `ocaml/src/data.ml`, `ocaml/src/compile/types.ml` and `ocaml/src/compile/check.ml` own the tables, generic substitution and checking. Operation implementations are indexed once. Preserved from the earlier prototype checkpoint.
- [x] Read a supplied flat `.spitout` inventory and resolve preservation and `many` aggregation with deterministic ordering and minimum checks. `ocaml/src/plan/inventory.ml` reads identities; `ocaml/src/plan/resolve.ml` groups symbol keys, matches input ports and builds indexed artifacts and jobs. Preserved from the earlier prototype checkpoint.
- [x] Bind v1 paths and command arguments and write version-18 `.spitdag` output for the existing Python runner. `ocaml/src/bind/paths.ml`, `ocaml/src/bind/command.ml` and `ocaml/src/output/dag.ml` keep path rules, command words and JSON output separate. The weather mock command strings use literal `\n` escapes for awk, which emits actual newlines. Preserved from the earlier prototype checkpoint.
- [x] Execute the v1 experiment through the runner. `ocaml/tests/compiler.py` passes 41 CLI cases, executes six weather jobs producing site means of `20.00` and `60.00`, and verifies that a second run reuses all six. Reordering inventory records gives identical DAG bytes. Inventory runs of 1,000 and 4,000 records took 0.132 and 0.536 seconds, including the CLI and JSON decoding. The v1 DAG also matches the existing compiler's binary output after translating pipeline syntax and normalizing generator/provenance and equivalent filesystem paths. No Rust source was inspected. Preserved from the earlier prototype checkpoint.

The compiler experiment covers optional and strict types, generic signatures and declared arities, imports and composite expansion, forward product references, scoped defaults, file sources, ordinary matching, composed selectors and parallel collections, named outputs, command quoting and stream redirection, verification scripts, path properties, dimension projection, join/list/scalar calls, resources, anchoring and calculations. Gates, tools, folder/sidecar binding, filesystem discovery, recipes, rich inventory sections, editor output and precise DAG provenance remain unsupported. Imported source companion closure and external-library script folders need further work. Compilation does not check source-file existence before handing the DAG to the runner.

### OCaml port

- [x] Port the span readers, entry layout, declaration grammar, diagnostics and lowering to dependency-free OCaml modules. The F# prototype remains available as a comparison baseline. Preserved from the earlier prototype checkpoint.
- [x] Enforce the agreed syntax: explicit product calls, qualified types, braced command functions, `$` output properties and bracketed input dimensions, and no default commands. Extension settings use the same extension type as declarations; entity formats and dimension renaming have distinct cases. Preserved from the earlier prototype checkpoint.
- [x] Preserve the reference's scoped defaults and gate removal, composed selectors, first/last selection and composite-body restrictions in the parsed data. Repeated stage blocks merge, singleton settings report duplicates, and sources support folders and omitted descriptions. Each scope stores calls once; generated artifacts refer to their call and output by index. Parsing does not resolve imports, inherit effective settings, check reference/type compatibility, plan jobs or expand command values. Those behaviors belong to subsequent compilation stages. Preserved from the earlier prototype checkpoint.
- [x] Compile and run regression checks over every mock, source locations, malformed input, nesting, scopes and the agreed syntax. All five full models match F# after normalizing representation differences. `./ocaml/parser.sh --test` also checks 100,000 products, 2,000 deterministic malformed inputs, CRLF and preserved script whitespace. CLI stdin, partial output and diagnostic exit status pass. With conventional ocamlformat layout, production parser OCaml is 2,093 lines, including model, diagnostic data, template parsing and JSON output, with 323 lines of parser tests. The additional compiler pilot is 1,659 lines. The earlier 1,117-line parser count used compressed formatting and is not a fair comparison of succinctness. Preserved from the earlier prototype checkpoint.

The OCaml files separate data (`ocaml/src/syntax/ast.ml`), error rendering (`ocaml/src/syntax/diagnostics.ml`), span readers (`ocaml/src/syntax/span.ml`), declaration grammar (`ocaml/src/syntax/grammar.ml`), entry layout (`ocaml/src/syntax/layout.ml`), command interpolation syntax (`ocaml/src/syntax/template.ml`), scope parsing and lowering (`ocaml/src/syntax/parser.ml`), JSON values and encoding (`ocaml/src/output/json.ml`), syntax inspection (`ocaml/src/output/syntax_json.ml`) and CLI execution (`ocaml/bin/parse.ml`). Spans share the original source and use byte offsets with exclusive ends. `Source` indexes line starts for diagnostics and reads stored script lines without fabricating layout entries. `Pattern` parses located path and entity formats; binding selects rules once per product. `Scope` assembles and merges scopes independently of source parsing. `Data.program` holds imported files, libraries and composite contracts outside the syntax model. The template reader preserves literal spans and distinguishes ports, built-ins, output properties, input dimension projections and function calls; it does not read artifact files or substitute job values.

- [x] R Direct line consumption, distinct entry content cases, one keyword table and shared gate validation. `SourceSpan` readers replace character-depth scans, string splitting and header concatenation. The five mock outputs match the baseline byte for byte. Script comments survive at the beginning and end of a body, and comment-only scripts retain their text. Scratch cases also check nested generic arguments, multiline signatures and gate validation on parameters. Model declarations and diagnostic data have their own files. Preserved from the earlier prototype checkpoint.
- [x] Scope ownership, required diagnostic spans and consolidated dispatch. Scratch checks cover dedented continuations crossing a stage boundary, valid multiline stage calls, exact locations for empty and repeated gate annotations, and invalid headers and contexts. The five mock outputs match the baseline byte for byte. Parser size is 757 lines, with 1,109 across the three F# files. Preserved from the earlier prototype checkpoint.
- [x] Separate gate validation from setting construction. Validated annotations keep their spans and grouped uses. `gatesOn` constructs settings directly from them. The flatten-and-regroup round trip is removed. Preserved from the earlier prototype checkpoint.
- [x] Full function appraisal and responsibility refactor. Structural scanning and type grammar have separate readers. Parsed declarations retain annotations, settings and product fan-out are constructed during lowering, and statement dispatch contains reader selection. Exact models for all five mocks match the pre-refactor version. Persistent checks cover grammar, locations, normalization, scope boundaries and nesting. Production F# has 1,295 lines, with 136 lines of regression checks. Preserved from the earlier prototype checkpoint.
- [x] D1 Continuation and command bodies. `readUntil` reads a header through matched groups, including groups spanning physical lines. `takeBody` then consumes the indented body. Indented lines under a line that neither ends with `:` nor leaves a bracket open are an error, so an over-indented `product b` is reported instead of swallowed. `command`, `verify`, `check` and `screen` take a script: on the colon's line for one line, or as the lines under `name:`, kept as written by `script`, with line breaks, blank lines, `#` lines and relative indentation. A long command continues the shell's way, with `\`. v3's `clean` moved under `clean:` with `\`, and its `what_if(` call has its `)` on its own line again. The trailing-colon message from `values` is gone, since a value can no longer have lines under it.
- [x] D2 Comments. A `#` line is a comment at the level of entries and part of the script inside a script. A `#` outside quotes partway through any other line is an error: "a comment goes on a line of its own".
- [x] D3 One meaning for the colon. Text before the colon is the name and text after it the value. `valueOnly` handles `types`, `root`, `output` and `dimensions`, which take only a value (`root: ../network`). `nameOnly` handles `type`, `tool` and `anchored`, which take only a name. The owning branches of `readStatement` choose these readers. A mistake quotes what was written: `root C:/data` gives "`root` takes a value and no name: write `root: C:/data`". `use` is `use: path` or `use alias: path [names]`. The entry header keeps the keyword spelling and the text after it for these messages. The mocks' value-only lines gained their colons.
- [x] D4 Modifiers by shape. `describe` reads a description in a fixed order, `Type .ext / [dims] @gates`, each part at most once; anything out of place or repeated is an error that gives the order. `Shape` is `File of Ext option | Folder | FolderFrom | View`, so a folder can't have an extension, and `Output` lost its separate `Ext`. A description has one `Placement` (`[dims]`, `beside x [suffix "s"]` or `here`); a source defaults to `Dims []`, an output takes only `beside`. `beside here` is an error because `readPlacement` checks the name with `checkName`. Each gate annotation is written once (`@check(c, d)`), and a gate both applied and turned off on one line is an error. The mocks needed no change.
- [x] D5 Empty and reserved names. Every name passes `checkName`: it may not be empty or a reserved word (`here`, `beside`, `suffix`, `from`, `view`, `default`, `many`), and it is letters, digits and `_`, starting with a letter or `_`. That covers settings' targets, sources, gates and their parameters, operations, parameters, outputs, products, stages, tools, `anchored` and `use` aliases. `check: nonempty` gives "`check` needs a name", and `default:` inside `path:` gives "`default` is a prefix, not a name: write `default path: ...`". `readTypeDef` also requires a type's name to start with a capital letter, so part of P2 is done.
- [x] D6 Gate kinds. `GateDef` is `Check of Test | Screen of Test | Review of name`, where a `Test` has a name, parameters and a script that `readGateDefinition` requires. A review takes no command and no parameters, since a person gives its verdicts. The prototype currently rejects negated defaults. This conflicts with the reference's stage inheritance rule and remains implementation work.
- [ ] D7 Apply the agreed default eligibility rule and complete setting representations. The language eligibility decision is settled.
- [ ] D8 Operation bodies
- [ ] D9 Jobs and references
- [ ] D10 Namespaces and fields
- [ ] D11 Settings that appear once
- [ ] D12 Smaller design points
- [x] P1 Consuming span readers match delimiters and reject trailing expression text. Comma lists reject leading, trailing and repeated commas. Calls, operation signatures, generic arguments and annotations use the shared readers. Quoted `->` inside an argument stays within that argument. Errors retain their original source spans, including on multiline input. Preserved from the earlier prototype checkpoint.
- [x] P2 Identifier and type-name validation. References validate namespace and name spans, named types start with capitals, variables and imported names are checked, and dimensions and tool uses share the identifier contract. Type-definition parameter errors point to the argument, including on multiline input. Qualified types remain part of D10.
- [x] P3 Errors as results. Invalid or overflowing `@min` values and empty gate annotations on sources, outputs and parameters produce line diagnostics and parsing continues.
- [ ] P4 Leftover special cases

## Function appraisal

The pass reviewed all 100 named callables in the preceding version, including local helpers, function aliases and builder methods. Anonymous callbacks were assessed with their enclosing functions. The table records the responsibility assessment and the resulting edit. Retained functions already have a coherent contract.

| Callable reviewed | Appraisal and outcome |
|---|---|
| `ResultBuilder.Bind` | Retained result sequencing. |
| `ResultBuilder.Return` | Retained success construction. |
| `ResultBuilder.ReturnFrom` | Retained the required diagnostic error type. |
| `readList` | Retained ordered, fail-fast collection reading. |
| `readList.loop` | Retained its tail-recursive accumulator. |
| `text` | Retained explicit string materialization. |
| `empty` | Retained the span emptiness predicate. |
| `slice` | Retained source-preserving subspan construction. |
| `drop` | Retained prefix consumption. |
| `startsAt` | Retained bounded ordinal token comparison. |
| `starts` | Retained the span-prefix adapter. |
| `trim` | Retained whitespace-boundary adjustment. |
| `at` | Retained construction of a required located diagnostic. |
| `readUntil` | Removed type-name and annotation heuristics. Its contract is structural scanning. |
| `readUntil.consume` | Retained local cursor and error propagation. |
| `readDelimited` | Retained matched delimiters and bounded nesting. |
| `commaList` | Reused `commaItems` with a raw-field reader. |
| `commaList.read` | Replaced the duplicate loop with the shared comma reader. |
| `words` | Retained whitespace-separated span reading. |
| `words.read` | Retained progress across whitespace and comment-only spans. |
| `requireEnd` | Retained rejection of unconsumed expression text. |
| `separated` | Retained structural separator consumption. |
| `applied` | Retained call-envelope reading and exact consumption. |
| `enclosed` | Retained bounded raw comma-list reading. |
| `splitEntry` | Retained optional colon-envelope reading. |
| `lines` | Retained physical line spans and indentation. |
| `ignored` | Retained DSL trivia classification. |
| `takeBody` | Retained bounded body partitioning and explicit script policy. |
| `script` | Retained script span capture and indentation metadata. |
| `takesScript` | Retained the current lexical script policy. D7 owns changes to defaults. |
| `classifyHeader` | Replaced allocating colon comparisons with token checks. |
| `readHeader` | Retained keyword, prefix and grouping recognition. |
| `readEntry` | Delegated grammar-aware extent reading to `readLogicalHeader` and retained the scope limit. |
| `entries` | Retained sequence ownership and keyword-group expansion. |
| `single` | Retained the one-value contract used by existing setting forms. |
| `scriptLines` | Retained materialization of the accepted script and default forms. |
| `contentText` | Retained diagnostic content rendering. |
| `isIdentifier` | Retained accepted underscore names and corrected the diagnostic contract. |
| `checkName` | Reused it for references, tools, imports and dimensions. |
| `owner` | Retained the small diagnostic-context adapter. |
| `target` | Removed eager name materialization from valid paths. |
| `operationTarget` | Retained operation-target construction through the shared rule. |
| `artifactTarget` | Retained artifact-target construction through the shared rule. |
| `dimensionTarget` | Retained dimension-target construction through the shared rule. |
| `readRef` | Split and validated original source spans before materializing components. |
| `readGateUse` | Retained gate-use reading through the checked reference reader. |
| `readAnnotation` | Deferred diagnostic payload construction until failure. |
| `gateKind` | Retained the keyword-table lookup. |
| `readToolUse` | Added the shared identifier check. |
| `readType` | Used a consuming type-prefix reader and required the expected end. |
| `readOptionalType` | Consumed the type directly before modifier reading. |
| `readExtension` | Retained one extension-reader contract. |
| `readShape` | Retained shape reading and used span token comparisons. |
| `readPlacement` | Retained placement reading and checked dimension names. |
| `readPlacement.beside` | Retained checked beside-placement construction. |
| `readGateAnnotation` | Retained the nonempty gate-use contract and source span. |
| `readDescriptionGate` | Deferred invalid-description payloads until failure. |
| `validateGates` | Checked conflicts within each annotation and returned annotations intact. |
| `describe` | Used `readDescriptionPrefix` and exact consumption. |
| `gatesOn` | Moved its callers into lowering. |
| `readSource` | Returned an annotated source declaration. |
| `readOutput` | Replaced it with `readOutputPrefix`, returning annotations with the output. |
| `readMinimum` | Retained focused integer conversion and its located errors. |
| `readParameterAnnotation` | Retained classification and deferred error payloads. |
| `readParam` | Replaced it with `readParamPrefix`, with no enclosing operation argument. |
| `readOperation` | Returned a parsed operation with annotated members. |
| `readIteration` | Deferred error payloads and checked dimension names. |
| `readArg` | Retained one argument-reader contract. |
| `readBindingDescription` | Retained the binding-specific description restrictions. |
| `readProduct` | Returned one binding and deferred artifact fan-out. |
| `readUse` | Retained import reading and checked selected names. |
| `readTypeDef` | Validated parameter syntax while argument spans are available. |
| `readTypeDef.param` | Replaced it with `readTypeParameter`, preserving argument locations. |
| `readDefaultGate` | Validated each use before its span was discarded. |
| `readGateDefinition` | Delegated review and executable-test contracts to their own readers. |
| `valueOnly` | Retained the explicit value-only envelope contract. |
| `nameOnly` | Retained the explicit name-only envelope contract. |
| `scopeOf` | Retained four clear projections over lowered items. |
| `validateContext` | Retained validation before declaration reading. |
| `readScope` | Returned source-ordered parsed statements and diagnostics. |
| `readStatement` | Reduced it to an exhaustive reader dispatcher. |
| `readStatement.ordinary` | Removed the output-protocol adapter. |
| `readStatement.item` | Removed the nested item-list adapter. |
| `readStatement.setting` | Removed the nested setting adapter. |
| `readStatement.namedValue` | Moved the shared contract into `readValueSetting`. |
| `readStatement.scriptSetting` | Moved script-setting reading into `readScriptSetting`. |
| `readStatement.pairs` | Extracted `readResources`. |
| `readStatement.gate` | Moved gate-form selection into `readGate`. |
| `readBody` | Returned parsed bodies and enforced scope nesting depth. |
| `pipelineOf` | Retained assembly. D11 owns singleton-setting policy. |
| `parse` | Composed layout, parsed declarations, lowering and pipeline assembly. |
| `show` | Used `%0A` to preserve literal whitespace in compact output. |
| `printScope` | Retained scoped model rendering. |
| `printPipeline` | Retained pipeline and diagnostic rendering. |
| `keywords.definition` | Retained top-level keyword-record construction. |
| `keywords.scoped` | Retained scoped keyword-record construction. |
| `keywords.gate` | Retained gate keyword-record construction. |
| `gateWord` | Retained diagnostic keyword lookup. |
| `renderProblem` | Retained centralized error rendering and corrected identifier wording. |
| `diagnosticLine` | Retained source-offset line lookup. |

The CLI policy and execution loop were also assessed. `run` owns file execution, and the direct-execution guard makes the parser usable through `#load`.

### Readers introduced by the refactor

Each added callable was also assessed against one contract.

| Callable | Contract |
|---|---|
| `isToken` | Compare an entire span with a literal. |
| `trivia` | Consume whitespace and whole-line comments. |
| `namePrefix` | Consume an identifier span and return its remainder. |
| `commaItems` | Consume comma-separated values through the supplied reader and ending boundary. |
| `commaItems.ended` | Recognize the selected ending boundary. |
| `commaItems.finish` | Consume that ending boundary. |
| `commaItems.read` | Advance through items, preserving empty-field errors. |
| `typeName` | Validate a named type identifier. |
| `readTypePrefix` | Consume one type expression, including its generic arguments. |
| `typeStarts` | Recognize the optional type prefix supported by the grammar. |
| `requireTypeBoundary` | Require separation between a type and its modifiers. |
| `horizontal` | Consume spaces and tabs within a physical header. |
| `readLogicalHeader` | Locate a logical header through the relevant grammar readers. |
| `readLogicalHeader.optionalType` | Consume an optional type in the current physical header. |
| `readDescriptionPrefix` | Consume description fields through the next comma boundary. |
| `readOutputPrefix` | Consume one output declaration with its attached annotations. |
| `readParamPrefix` | Consume one parameter declaration with its attached annotations. |
| `readTypeParameter` | Require a type variable while its argument span is available. |
| `readDefaultGate.readUse` | Validate one default gate use at its own location. |
| `readIteration.readPair` | Read a dimension/value restriction. |
| `readReview` | Read the non-executable review declaration contract. |
| `readTest` | Read an executable gate declaration. |
| `readResources` | Read resource key/value pairs. |
| `readValueSetting` | Read a targeted value setting. |
| `readScriptSetting` | Read a targeted script setting. |
| `readAnchored` | Read the anchored setting's named and default forms. |
| `readGate` | Select a gate definition or a default gate application. |
| `readScope.read` | Apply context validation before a statement reader. |
| `readStage` | Assemble a named parsed stage and its body. |
| `readOperationDeclaration` | Assemble a parsed operation signature and its optional body. |
| `lowerScope` | Lower source-ordered entries and collect their diagnostics. |
| `lowerStatement` | Construct final model items from one parsed declaration. |
| `run` | Execute the parser CLI over files. |

### Overall appraisal

The overlap came from grammar context inside structural scanning, declarations returning side lists of settings, and statement dispatch assembling bodies and adapting several result shapes. The revised flow is original source spans, entry envelopes, parsed declarations, then lowering into the final model. Each reader returns its own data and a bounded remainder where needed. Validation retains source spans until its checks finish.

Small predicates, result sequencing, data constructors and projections remain small. The refactor adds type grammar and parsed-declaration records, so production line count increases. The resulting contracts can be tested independently. `mock_parser_tests.fsx` checks source positions, lossless display, generic headers, comma fields, scope boundaries, diagnostic order and normalization.
