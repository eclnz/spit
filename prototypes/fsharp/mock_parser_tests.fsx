#load "mock_parser.fsx"

open MockModel
open MockDiagnostics
open Mock_parser

let span source = { Source = source; Start = 0; Finish = source.Length }
let success = function Ok value -> value | Error diagnostic -> failwith (renderProblem gateWord diagnostic.Problem)
let failure = function Error _ -> () | Ok _ -> failwith "expected a diagnostic"
let validPipeline source =
    let pipeline, diagnostics = parse source
    if not diagnostics.IsEmpty then failwithf "%A" diagnostics
    pipeline

for file in [ "v1_mock.spit"; "v2_mock.spit"; "v3_mock.spit"; "v4_mock.spit"; "stats_mock.spit" ] do
    System.IO.File.ReadAllText (System.IO.Path.Combine (__SOURCE_DIRECTORY__, "../../ocaml/examples", file)) |> validPipeline |> ignore

let inside, remaining = readDelimited 0 '(' ')' (span "(x)[y]") |> success
assert (text inside = "x" && text remaining = "[y]")
for source in [ ""; " "; "\n" ] do assert (commaList (span source) |> success |> List.isEmpty)
for source in [ ","; ",x"; "x,"; "x,,y"; "x, ,y" ] do commaList (span source) |> failure
assert (commaList (span "\"x,y\", g(a,b)") |> success |> List.map text = [ "\"x,y\""; "g(a,b)" ])
for source in [ "([x))"; "(x]"; "([x)"; "(x" ] do readDelimited 0 '(' ')' (span source) |> failure

do
    let source = "product malformed = f([x))\nsource kept: Mean\n"
    let pipeline, diagnostics = parse source
    assert (pipeline.Global.Artifacts |> List.map _.Name = [ "kept" ])
    match diagnostics with
    | [ { Span = location; Problem = UnexpectedClosing ')' } ] ->
        assert (text location = ")" && location.Start = source.IndexOf ')')
        assert (System.Object.ReferenceEquals(location.Source, source))
    | other -> failwithf "unexpected bracket diagnostics %A" other
    for expression in [ "f(x,,y)"; "f(,)"; "f(x,)"; "f(,x)" ] do
        let pipeline, diagnostics = parse ("product bad = " + expression + "\nsource kept: Mean")
        assert (diagnostics.Length = 1)
        assert (pipeline.Global.Artifacts |> List.map _.Name = [ "kept" ])
    for expression in [ "f()"; "f(x)"; "f(x, y)" ] do validPipeline ("product accepted = " + expression) |> ignore

do
    let pipeline = validPipeline "operation tested(x: Mean @check(test(\"a->b\"))) -> Mean"
    assert (pipeline.Global.Operations |> List.map _.Name = [ "tested" ])
    assert (pipeline.Global.Settings |> List.exists (function
        | Setting.Gate (_, InputOf ("tested", "x"), [ gate ]) -> gate.Args = [ "\"a->b\"" ]
        | _ -> false))
    for value in [ "a<b"; "A<B" ] do validPipeline ("source raw: Mean @check(compare(" + value + "))") |> ignore
    validPipeline "calculate lower: left < right\npath raw: folder<name/file\nproduct comparison = f(a<b)" |> ignore

do
    let source = "operation tested(\n    x: Mean,\n    y: Mean,,\n    z: Mean\n) -> Mean\nsource kept: Mean"
    let pipeline, diagnostics = parse source
    assert (diagnostics.Length = 1 && diagnosticLine diagnostics.Head = 3)
    assert (pipeline.Global.Artifacts |> List.map _.Name = [ "kept" ])
    let source = "operation convert(\n    input: Mean @check()\n) -> Mean"
    let _, diagnostics = parse source
    assert (diagnostics.Length = 1 && diagnosticLine diagnostics.Head = 2)
    assert (text diagnostics.Head.Span = "@check()")
    let source = "operation convert(\n    input: Mean @check(a)\n    @check(b)\n) -> Mean"
    let _, diagnostics = parse source
    assert (diagnostics.Length = 1 && diagnosticLine diagnostics.Head = 3)
    assert (text diagnostics.Head.Span = "@check(b)")
    let source = "type Pair<\n    $A,\n    Mean\n>"
    let _, diagnostics = parse source
    assert (diagnostics.Length = 1 && diagnosticLine diagnostics.Head = 3)
    assert (text diagnostics.Head.Span = "Mean")

do
    let source = "type Pair<\n    $Left,\n    $Right\n>\nsource raw: Pair<\n    Mean,\n    Box<Mean>\n> .csv"
    let pipeline = validPipeline source
    assert (pipeline.TypeDefs = [ { Name = "Pair"; Params = [ "Left"; "Right" ] } ])
    match pipeline.Global.Artifacts.Head.Origin with
    | Found { Type = Some (Named ("Pair", [ Named ("Mean", []); Named ("Box", [ Named ("Mean", []) ]) ])) } -> ()
    | other -> failwithf "unexpected generic source %A" other
    validPipeline "operation f(x:\n# type comment\n Pair<Mean, Box<Mean>>) -> Pair<Mean, Mean>" |> ignore
    for value in [ "Mean>"; "Mean.csv" ] do
        let _, diagnostics = parse ("source malformed: " + value)
        assert (not diagnostics.IsEmpty)
    readRef (span "one.two.three") |> failure
    assert (readRef (span "one.two") |> success = { Namespace = Some "one"; Name = "two" })
    assert (checkName "an artifact" (span "_name") |> success = "_name")

do
    let pipeline = validPipeline "command script:\n    # first\n    echo   hi\n\n    # last\nsource kept: Mean"
    assert (pipeline.Global.Settings = [ Setting.Command (NamedOperation "script", [ "# first"; "echo   hi"; ""; "# last" ]) ])
    assert (show "a  b" <> show "a b")
    let pipeline = validPipeline "operation split(x: Mean @check(okay)) -> (left: Mean @check(okay), right: Mean)\nproduct a, b = split(input)"
    assert (pipeline.Global.Artifacts |> List.map (fun artifact -> match artifact.Origin with Made (_, index, _) -> index | _ -> -1) = [ 0; 1 ])
    assert (pipeline.Global.Settings.Length = 2)

do
    let source = "stage scope:\n    product answer = f(\narg\n)\nsource kept: Mean"
    let pipeline, diagnostics = parse source
    assert (pipeline.Global.Stages.Head.Scope.Artifacts.IsEmpty)
    assert (pipeline.Global.Artifacts |> List.map _.Name = [ "kept" ])
    assert (diagnostics |> List.exists (fun diagnostic -> diagnostic.Problem = UnclosedBracket))
    assert (diagnostics |> List.filter (fun diagnostic -> diagnosticLine diagnostic = 3) |> List.length = 1)
    let pipeline = validPipeline "stage scope:\n    product answer = f(\n        arg\n    )\nsource kept: Mean"
    assert (pipeline.Global.Stages.Head.Scope.Artifacts |> List.map _.Name = [ "answer" ])
    let source = "unknown first\nstage scope:\n    unknown child\n    source kept: Mean\nunknown last"
    let pipeline, diagnostics = parse source
    assert (diagnostics |> List.map diagnosticLine = [ 1; 3; 5 ])
    assert (pipeline.Global.Stages.Head.Scope.Artifacts |> List.map _.Name = [ "kept" ])

do
    let source = "operation deep(x: " + String.replicate 150 "Box<" + "Mean" + String.replicate 150 ">" + ") -> Mean\nsource kept: Mean"
    let pipeline, diagnostics = parse source
    assert (diagnostics.Length = 1 && diagnostics.Head.Problem = NestingLimit)
    assert (pipeline.Global.Artifacts |> List.map _.Name = [ "kept" ])
    let stages = [ for depth in 0..140 -> String.replicate (depth * 4) " " + "stage s" + string depth + ":" ]
    let pipeline, diagnostics = parse (String.concat "\n" stages + "\nsource kept: Mean")
    assert (diagnostics.Length = 1 && diagnostics.Head.Problem = NestingLimit)
    assert (pipeline.Global.Artifacts |> List.map _.Name = [ "kept" ])

do
    let source = "source raw: Mean @check(nonempty)"
    let parsed = source |> lines |> readScope 0
    match parsed.Entries with
    | [ Ok (SourceDeclaration declaration) ] ->
        assert (declaration.Gates.Length = 1)
        assert (text declaration.Gates.Head.Span = "@check(nonempty)")
    | other -> failwithf "unexpected parsed source %A" other
    let items, diagnostics = lowerScope parsed
    assert diagnostics.IsEmpty
    assert (items.Length = 2)
    let source = "operation average(x: Mean @check(nonempty)) -> Mean @review(looked)"
    let parsed = source |> lines |> readScope 0
    match parsed.Entries with
    | [ Ok (OperationDeclaration declaration) ] ->
        assert (declaration.Params.Head.Gates.Head.Kind = GateKind.Check)
        assert (declaration.Outputs.Head.Gates.Head.Kind = GateKind.Review)
    | other -> failwithf "unexpected parsed operation %A" other
    let items, diagnostics = lowerScope parsed
    assert diagnostics.IsEmpty
    assert (items.Length = 3)

printfn "Parser checks passed."
