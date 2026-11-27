#load "mock_model.fsx"
#load "mock_diagnostics.fsx"

open MockModel
open MockDiagnostics

type ResultBuilder() =
    member _.Bind(value, next) = Result.bind next value
    member _.Return(value) = Ok value
    member _.ReturnFrom(value: Result<_, Diagnostic>) = value

let reading = ResultBuilder()

let readList read values =
    let rec loop acc = function
        | [] -> Ok (List.rev acc)
        | first :: rest ->
            match read first with
            | Ok value -> loop (value :: acc) rest
            | Error problem -> Error problem
    loop [] values

let text span = span.Source.Substring(span.Start, span.Finish - span.Start)
let empty span = span.Start = span.Finish
let slice start finish span = { span with Start = start; Finish = finish }
let drop count span = { span with Start = span.Start + count }
let startsAt (token: string) offset span =
    token.Length <= span.Finish - offset
    && System.String.CompareOrdinal(span.Source, offset, token, 0, token.Length) = 0
let starts token span = startsAt token span.Start span
let isToken token span = span.Finish - span.Start = String.length token && starts token span
let maxNesting = 128

let trim span =
    let mutable start, finish = span.Start, span.Finish
    while start < finish && System.Char.IsWhiteSpace span.Source.[start] do start <- start + 1
    while finish > start && System.Char.IsWhiteSpace span.Source.[finish - 1] do finish <- finish - 1
    slice start finish span

let at span problem = Error { Span = span; Problem = problem }

let rec readUntil depth stops input =
    let mutable cursor = input.Start
    let mutable problem = None
    let mutable first, finish = input.Finish, input.Start
    while cursor < input.Finish && problem.IsNone
          && not (stops |> List.exists (fun token -> startsAt token cursor input)) do
        let beginAt = cursor
        let c = input.Source.[cursor]
        let consume result =
            match result with
            | Ok (_, rest) -> cursor <- rest.Start
            | Error error -> problem <- Some error
        match c with
        | '"' | '\'' ->
            let quote = cursor
            cursor <- cursor + 1
            while cursor < input.Finish && input.Source.[cursor] <> c do
                cursor <- cursor + (if input.Source.[cursor] = '\\' && cursor + 1 < input.Finish then 2 else 1)
            if cursor = input.Finish then problem <- Some { Span = slice quote (quote + 1) input; Problem = UnclosedQuote c }
            else cursor <- cursor + 1
        | '(' | '[' ->
            consume (readDelimited (depth + 1) c (if c = '(' then ')' else ']') (slice cursor input.Finish input))
        | ')' | ']' -> problem <- Some { Span = slice cursor (cursor + 1) input; Problem = UnexpectedClosing c }
        | '#' ->
            let mutable before = cursor - 1
            while before >= 0 && input.Source.[before] <> '\n' && System.Char.IsWhiteSpace input.Source.[before] do
                before <- before - 1
            if before >= 0 && input.Source.[before] <> '\n' then problem <- Some { Span = slice cursor (cursor + 1) input; Problem = InlineComment }
            else
                while cursor < input.Finish && input.Source.[cursor] <> '\n' do cursor <- cursor + 1
        | _ ->
            cursor <- cursor + 1
        if c <> '#' && not (System.Char.IsWhiteSpace c) then
            first <- min first beginAt
            finish <- cursor
    match problem with
    | Some error -> Error error
    | None ->
        let consumed = if finish < first then slice cursor cursor input else slice first finish input
        Ok (consumed, slice cursor input.Finish input)

and readDelimited depth opening closing input = reading {
    if depth > maxNesting then return! at input NestingLimit
    elif not (starts (string opening) input) then return! at input (MissingDelimiters (opening, closing, text input))
    else
        let! inside, rest = readUntil depth [ string closing ] (drop 1 input)
        if empty rest then return! at (slice input.Start (input.Start + 1) input) UnclosedBracket
        else return inside, drop 1 rest
}

let words input =
    let rec read acc remaining = reading {
        let remaining = trim remaining
        if empty remaining then return List.rev acc
        else
            let! word, rest = readUntil 0 [ " "; "\t"; "\r"; "\n" ] remaining
            let acc = if empty word then acc else word :: acc
            return! read acc rest
    }
    read [] input

let requireEnd remaining =
    if empty (trim remaining) then Ok () else at remaining (UnexpectedText (text remaining))

let separated separator input = reading {
    let! left, rest = readUntil 0 [ separator ] input
    if empty rest then return! at input (MissingSeparator (separator, text input))
    else return left, trim (drop separator.Length rest)
}

let splitEntry input = reading {
    let! head, rest = readUntil 0 [ ":" ] input
    return head, if empty rest then None else Some (trim (drop 1 rest))
}

let reserved = set [ "here"; "beside"; "suffix"; "from"; "view"; "default"; "many" ]

let isIdentifier (name: string) =
    name <> ""
    && (System.Char.IsLetter name.[0] || name.[0] = '_')
    && name |> Seq.forall (fun c -> System.Char.IsLetterOrDigit c || c = '_')

let checkName owner span =
    let name = text span
    if name = "" then at span (MissingName owner)
    elif name = "default" then at span DefaultUsedAsName
    elif reserved.Contains name then at span (ReservedName name)
    elif not (isIdentifier name) then at span (InvalidName name)
    else Ok name

let readRef span = reading {
    let dot = span.Source.IndexOf('.', span.Start, span.Finish - span.Start)
    if dot < 0 then
        let! name = checkName "a reference" span
        return { Namespace = None; Name = name }
    else
        let! space = checkName "a namespace" (slice span.Start dot span)
        let! name = checkName "a reference" (slice (dot + 1) span.Finish span)
        return { Namespace = Some space; Name = name }
}


let rec trivia input = reading {
    let input = trim input
    if starts "#" input then
        let! _, rest = readUntil 0 [ "\n" ] input
        return! trivia rest
    else return input
}

let namePrefix input =
    trivia input |> Result.map (fun input ->
        let mutable finish = input.Start
        while finish < input.Finish && (System.Char.IsLetterOrDigit input.Source.[finish] || input.Source.[finish] = '_') do
            finish <- finish + 1
        slice input.Start finish input, slice finish input.Finish input)

let commaItems closing reader input =
    let ended rest = match closing with Some token -> starts token rest | None -> empty rest
    let finish rest = match closing with Some token -> drop token.Length rest | None -> rest
    let rec read required acc remaining = reading {
        let! remaining = trivia remaining
        if ended remaining then
            if required then return! at remaining EmptyListItem
            else return List.rev acc, finish remaining
        elif empty remaining then return! at remaining UnclosedBracket
        else
            let! value, rest = reader remaining
            if rest.Start = remaining.Start then return! at rest EmptyListItem
            else
                let! rest = trivia rest
                if ended rest then return List.rev (value :: acc), finish rest
                elif starts "," rest then return! read true (value :: acc) (drop 1 rest)
                elif empty rest then return! at rest UnclosedBracket
                else return! at rest (UnexpectedText (text rest))
    }
    read false [] input

let commaList input = commaItems None (readUntil 0 [ "," ]) input |> Result.map fst

let applied input = reading {
    let! name, rest = readUntil 0 [ "(" ] input
    if empty rest then return name, []
    else
        let! inside, rest = readDelimited 0 '(' ')' rest
        do! requireEnd rest
        let! args = commaList inside
        return name, args
}

let enclosed opening closing input = reading {
    let! inside, rest = readDelimited 0 opening closing input
    do! requireEnd rest
    return! commaList inside
}

let typeName span =
    if empty span || not (System.Char.IsUpper span.Source.[span.Start]) then at span (InvalidTypeName (text span))
    else checkName "a type" span

let rec readTypePrefix depth input = reading {
    if depth > maxNesting then return! at input NestingLimit
    else
        let! input = trivia input
        let variable = starts "$" input
        let! name, rest = namePrefix (if variable then drop 1 input else input)
        let! name = if variable then checkName "a type variable" name else typeName name
        if variable then return Var name, rest
        elif starts "<" rest then
            let! args, rest = commaItems (Some ">") (readTypePrefix (depth + 1)) (drop 1 rest)
            return Named (name, args), rest
        else return Named (name, []), rest
}

let readType input = reading {
    let! value, rest = readTypePrefix 0 input
    do! requireEnd rest
    return value
}

let typeStarts input =
    not (empty input) && (System.Char.IsUpper input.Source.[input.Start] || starts "$" input)

let readOptionalType input = reading {
    let! input = trivia input
    if typeStarts input then
        let! value, rest = readTypePrefix 0 input
        return Some value, rest
    else return None, input
}

let requireTypeBoundary (typ: TypeExpr option) rest =
    if typ.IsSome && not (empty rest) && not (System.Char.IsWhiteSpace rest.Source.[rest.Start]) && not (starts "," rest) then
        at rest (UnexpectedText (text rest))
    else Ok ()

let horizontal input =
    let mutable start = input.Start
    while start < input.Finish && (input.Source.[start] = ' ' || input.Source.[start] = '\t') do start <- start + 1
    { input with Start = start }

let readLogicalHeader keyword input = reading {
    let optionalType rest =
        let rest = horizontal rest
        if typeStarts rest then readTypePrefix 0 rest |> Result.map snd else Ok rest
    let! tail =
        match keyword with
        | Keyword.Type when typeStarts input -> readTypePrefix 0 input |> Result.map snd
        | Keyword.Source | Keyword.Product -> reading {
            let! _, rest = readUntil 0 [ ":"; "="; "\r"; "\n" ] input
            if starts ":" rest then return! optionalType (drop 1 rest)
            else return rest
          }
        | Keyword.Operation -> reading {
            let! _, rest = readUntil 0 [ "("; "->"; "\r"; "\n" ] input
            let! rest =
                if starts "(" rest then readDelimited 0 '(' ')' rest |> Result.map snd
                else Ok rest
            let rest = horizontal rest
            if starts "->" rest then return! optionalType (drop 2 rest)
            else return rest
          }
        | _ -> Ok input
    let! _, rest = readUntil 0 [ "\r"; "\n" ] tail
    return trim (slice input.Start rest.Start input), rest
}

let lines (source: string) =
    Seq.unfold (fun start ->
        if start > source.Length then None
        else
            let newline = source.IndexOf('\n', start)
            let finish = if newline < 0 then source.Length else newline
            let span = { Source = source; Start = start; Finish = finish }
            let trimmed = trim span
            let indent = if empty trimmed then finish - start else trimmed.Start - start
            Some ({ Indent = indent; Span = span }, finish + 1)) 0
    |> Seq.toList

let ignored (line: Line) = let span = trim line.Span in empty span || starts "#" span

let takeBody raw indent (pending: Line list) =
    let body = pending |> List.takeWhile (fun line ->
        empty (trim line.Span) || (not raw && ignored line) || line.Indent > indent)
    body, List.skip body.Length pending

let script (body: Line list) fallback =
    let body = body |> List.rev |> List.skipWhile (fun line -> empty (trim line.Span)) |> List.rev
    match body with
    | [] -> Content.Script (slice fallback.Finish fallback.Finish fallback, 0)
    | first :: _ ->
        let finish = (List.last body).Span.Finish
        let indent = body |> List.filter (fun line -> not (empty (trim line.Span))) |> List.map _.Indent |> List.min
        Content.Script (slice first.Span.Start finish first.Span, indent)

let takesScript header =
    not header.Default
    && (match header.Rule.Form with
        | Script | Gate GateKind.Check | Gate GateKind.Screen -> true
        | _ -> false)

type EntryStart =
    | DefaultBlock
    | KeywordBlock of EntryHeader
    | KeywordLine of EntryHeader

let classifyHeader inheritedDefault rule rest =
    let header = { Rule = rule; Default = inheritedDefault; Rest = rest }
    if not (isToken ":" rest) then Ok (KeywordLine header)
    elif rule.Keyword = Keyword.Stage then at rest StageNeedsName
    else Ok (KeywordBlock header)

let rec readHeader inheritedDefault supplied input =
    match supplied with
    | Some rule -> classifyHeader inheritedDefault rule input
    | None -> reading {
        let! word, rest = readUntil 0 [ " "; "\t"; ":" ] input
        let rest = trim rest
        match text word with
        | "default" when isToken ":" rest -> return DefaultBlock
        | "default" -> return! readHeader true None rest
        | word ->
            match Map.tryFind word keywords with
            | Some rule -> return! classifyHeader inheritedDefault rule rest
            | None when input.Source.IndexOf(" = ", input.Start, input.Finish - input.Start, System.StringComparison.Ordinal) >= 0 ->
                return! at input (MissingBindingKeyword (text input))
            | None -> return! at input (UnknownKeyword (text input))
      }

let readEntry limit header (line: Line) (pending: Line list) =
    let raw = takesScript header
    let joined =
        if raw then Ok (header.Rest, slice header.Rest.Finish header.Rest.Finish header.Rest)
        else readLogicalHeader header.Rule.Keyword { header.Rest with Finish = limit }
    match joined with
    | Error problem -> Error problem, pending
    | Ok (span, _) ->
        let pending = pending |> List.skipWhile (fun line -> line.Span.Start < span.Finish)
        let body, remaining = takeBody raw line.Indent pending
        let entry = reading {
            let! head, value = splitEntry span
            let! content =
                match raw, value with
                | true, Some value when empty value -> Ok (script body value)
                | true, Some value when body |> List.forall ignored -> Ok (Content.Script (value, 0))
                | true, Some value -> at value (InlineScriptWithBody (text head, text value))
                | true, None -> at head (MissingScript (header.Rule.Word, text head))
                | false, Some value when empty value -> Ok (Content.Body body)
                | false, _ when body |> List.exists (ignored >> not) -> at span (UnexpectedIndent (text span))
                | false, Some value -> Ok (Content.Value value)
                | false, None -> Ok Content.Bare
            return { Header = header; Head = head; Content = content }
        }
        entry, remaining

let rec entries supplied inheritedDefault (pending: Line list) =
    let limit = pending |> List.tryLast |> Option.map (fun line -> line.Span.Finish)
    pending
    |> List.unfold (fun pending ->
        match List.skipWhile ignored pending with
        | [] -> None
        | line :: rest ->
            match readHeader inheritedDefault supplied (trim line.Span) with
            | Error problem ->
                let _, remaining = takeBody false line.Indent rest
                Some ([ Error problem ], remaining)
            | Ok DefaultBlock ->
                let body, remaining = takeBody false line.Indent rest
                Some (entries None true body, remaining)
            | Ok (KeywordBlock header) ->
                let body, remaining = takeBody false line.Indent rest
                Some (entries (Some header.Rule) header.Default body, remaining)
            | Ok (KeywordLine header) ->
                let entry, remaining = readEntry (Option.defaultValue line.Span.Finish limit) header line rest
                Some ([ entry ], remaining))
    |> List.concat

let single e =
    let candidates =
        match e.Content with
        | Content.Value value -> [ value ]
        | Content.Body lines -> lines |> List.filter (ignored >> not) |> List.map (fun line -> trim line.Span)
        | _ -> []
    match candidates with
    | [ value ] -> Ok value
    | values -> at e.Head (ValueCount (text e.Head, values.Length))

let scriptLines e =
    match e.Content with
    | Content.Script (span, indent) when not (empty span) ->
        (text span).Split '\n' |> Array.toList |> List.map (fun line ->
            if System.String.IsNullOrWhiteSpace line then "" else line.[indent..].TrimEnd())
    | Content.Value value -> [ text value ]
    | Content.Body lines -> lines |> List.filter (ignored >> not) |> List.map (fun line -> text (trim line.Span))
    | _ -> []

let contentText e = String.concat " " (scriptLines e)

let owner e = sprintf "`%s`" e.Header.Rule.Word

let target every named e =
    match e.Header.Default, empty e.Head with
    | true, true -> Ok every
    | true, false -> at e.Head (NamedDefault (text e.Head))
    | false, _ when isToken "default" e.Head -> at e.Head (DefaultTarget (e.Header.Rule.Word, contentText e))
    | false, _ -> checkName (owner e) e.Head |> Result.map named

let operationTarget = target EveryOperation NamedOperation
let artifactTarget = target EveryArtifact NamedArtifact
let dimensionTarget = target EveryDimension NamedDimension


let readGateUse span = reading {
    let negated = starts "!" span
    let! name, args = applied (if negated then drop 1 span else span)
    let! gate = readRef name
    return { Gate = gate; Args = List.map text args; Negated = negated }
}

let readAnnotation invalid span = reading {
    if not (starts "@" span) then return! at span (invalid (text span))
    else
        let! name, args = applied (drop 1 span)
        return text name, args
}

let gateKind name =
    match Map.tryFind name keywords with
    | Some { Form = Gate kind } -> Some kind
    | _ -> None

let readToolUse span = reading {
    let negated = starts "!" span
    let! name = checkName "a tool" (if negated then drop 1 span else span)
    return { Tool = name; Used = not negated }
}

let readExtension = function
    | ext :: rest when starts "+." ext -> Some (Append (text (drop 1 ext))), rest
    | ext :: rest when starts "." ext -> Some (Replace (text ext)), rest
    | rest -> None, rest

let readShape tokens =
    match readExtension tokens with
    | None, slash :: from :: name :: rest when isToken "/" slash && isToken "from" from ->
        checkName "`from`" name |> Result.map (fun name -> FolderFrom name, rest)
    | None, slash :: view :: rest when isToken "/" slash && isToken "view" view -> Ok (View, rest)
    | None, slash :: rest when isToken "/" slash -> Ok (Folder, rest)
    | Some _, slash :: _ when isToken "/" slash -> at slash FolderExtension
    | ext, rest -> Ok (File ext, rest)

let readPlacement =
    let beside name suffix rest =
        checkName "`beside`" name |> Result.map (fun name -> Some (Beside { Of = name; Suffix = suffix }), rest)
    function
    | dims :: rest when starts "[" dims ->
        enclosed '[' ']' dims |> Result.bind (readList (checkName "a dimension"))
        |> Result.map (fun dims -> Some (Dims dims), rest)
    | word :: name :: suffix :: value :: rest when isToken "beside" word && isToken "suffix" suffix ->
        beside name (Some ((text value).Trim '"')) rest
    | word :: name :: rest when isToken "beside" word -> beside name None rest
    | word :: rest when isToken "here" word -> Ok (Some Here, rest)
    | rest -> Ok (None, rest)

let readGateAnnotation span kind args =
    if List.isEmpty args then at span (EmptyGateAnnotation kind)
    else readList readGateUse args |> Result.map (fun uses -> { Span = span; Kind = kind; Uses = uses })

let readDescriptionGate span = reading {
    let! name, args = readAnnotation DescriptionOrder span
    match gateKind name with
    | Some kind -> return! readGateAnnotation span kind args
    | None -> return! at span (DescriptionOrder (text span))
}

let validateGates annotations =
    let duplicate =
        annotations |> List.groupBy _.Kind |> List.tryPick (function
            | _, _ :: repeated :: _ -> Some repeated
            | _ -> None)
    let conflict annotation =
        annotation.Uses
        |> List.groupBy _.Gate
        |> List.tryPick (fun (gate, uses) ->
            if List.exists _.Negated uses && List.exists (_.Negated >> not) uses then Some (annotation.Span, gate.Name)
            else None)
    match duplicate with
    | Some repeated -> at repeated.Span (RepeatedGateAnnotation repeated.Kind)
    | None ->
        match List.tryPick conflict annotations with
        | Some (span, name) -> at span (ContradictedGate name)
        | None -> Ok annotations

let readDescriptionPrefix input = reading {
    let! typ, rest = readOptionalType input
    do! requireTypeBoundary typ rest
    let! modifiers, remaining = readUntil 0 [ "," ] rest
    let! tokens = words modifiers
    let! shape, tokens = readShape tokens
    let! placement, tokens = readPlacement tokens
    let! annotations = readList readDescriptionGate tokens
    let! gates = validateGates annotations
    return { Type = typ; Shape = shape; Placement = placement; Gates = gates }, remaining
}

let describe input = reading {
    let! description, rest = readDescriptionPrefix input
    do! requireEnd rest
    return description
}

let readSource e = reading {
    let! name = checkName (owner e) e.Head
    let! span = single e
    let! description = describe span
    match description.Shape with
    | File ext ->
        let found = { Type = description.Type; Ext = ext; Placement = defaultArg description.Placement (Dims []) }
        return { Value = { Name = name; Origin = Found found }; Gates = description.Gates }
    | _ -> return! at span (FolderSource name)
}

let readOutputPrefix input = reading {
    let! head, afterName = namePrefix input
    let afterName = horizontal afterName
    let! name, rest =
        if starts ":" afterName then checkName "an output" head |> Result.map (fun name -> Some name, drop 1 afterName)
        else Ok (None, input)
    let! description, remaining = readDescriptionPrefix rest
    let! beside =
        match description.Placement with
        | None -> Ok None
        | Some (Beside beside) -> Ok (Some beside)
        | Some _ -> at rest OutputPlacement
    let output = { Name = name; Type = description.Type; Shape = description.Shape; Beside = beside }
    return { Value = output; Gates = description.Gates }, remaining
}

type ParameterAnnotation =
    | ParameterConstraint of Constraint
    | ParameterGates of GateAnnotation

let readMinimum number =
    try Ok (ParameterConstraint (Min (int (text number))))
    with
    | :? System.FormatException
    | :? System.OverflowException -> at number (InvalidMinimum (text number))

let readParameterAnnotation span = reading {
    let! name, args = readAnnotation InvalidParameterAnnotation span
    match name, args, gateKind name with
    | "min", [ number ], _ -> return! readMinimum number
    | _, _, Some kind ->
        let! gate = readGateAnnotation span kind args
        return ParameterGates gate
    | _ -> return! at span (InvalidParameterAnnotation (text span))
}

let readParamPrefix input = reading {
    let! head, rest = readUntil 0 [ ":"; "," ] input
    let! name = checkName "a parameter" head
    if not (starts ":" rest) then
        return { Value = { Name = name; Many = false; Type = None; Constraints = [] }; Gates = [] }, rest
    else
        let! value = trivia (drop 1 rest)
        let! first, afterFirst = namePrefix value
        let many = isToken "many" first
        let! typ, rest = readOptionalType (if many then afterFirst else value)
        do! requireTypeBoundary typ rest
        let! annotations, remaining = readUntil 0 [ "," ] rest
        let! tokens = words annotations
        let! annotations = readList readParameterAnnotation tokens
        let constraints = annotations |> List.choose (function ParameterConstraint value -> Some value | _ -> None)
        let! gates =
            annotations
            |> List.choose (function ParameterGates annotation -> Some annotation | _ -> None)
            |> validateGates
        return { Value = { Name = name; Many = many; Type = typ; Constraints = constraints }; Gates = gates }, remaining
}

let readOperation span = reading {
    let! name, rest = readUntil 0 [ "("; "->" ] span
    let! name = checkName "`operation`" name
    let! parameters, rest =
        if starts "(" rest then readDelimited 0 '(' ')' rest
        else Ok (slice rest.Start rest.Start rest, rest)
    let! parameters, _ = commaItems None readParamPrefix parameters
    let rest = trim rest
    if not (starts "->" rest) then return! at rest (MissingSeparator ("->", text span))
    else
        let results = trim (drop 2 rest)
        let! outputs =
            if starts "(" results then reading {
                let! inside, rest = readDelimited 0 '(' ')' results
                do! requireEnd rest
                let! outputs, _ = commaItems None readOutputPrefix inside
                return outputs
            }
            else reading {
                let! output, rest = readOutputPrefix results
                do! requireEnd rest
                return [ output ]
            }
        return { Name = name; Params = parameters; Outputs = outputs; Body = None }
}

let readIteration span = reading {
    let! name, args = readAnnotation InvalidArgumentAnnotation span
    match name, args with
    | "vary", dims -> return! readList (checkName "a dimension") dims |> Result.map Vary
    | "each", [ dim ] -> return! checkName "a dimension" dim |> Result.map Each
    | "same", dims -> return! readList (checkName "a dimension") dims |> Result.map Same
    | "where", pairs ->
        let readPair pair = reading {
            let! key, value = separated "=" pair
            let! key = checkName "a dimension" key
            return key, text value
        }
        let! pairs = readList readPair pairs
        return Where pairs
    | _ -> return! at span (InvalidArgumentAnnotation (text span))
}

let readArg span = reading {
    let! tokens = words span
    match tokens with
    | [ value ] -> return { Value = text value; Iteration = None }
    | [ value; annotation ] ->
        let! iteration = readIteration annotation
        return { Value = text value; Iteration = Some iteration }
    | _ -> return! at span (InvalidArgument (text span))
}

let readBindingDescription span = reading {
    let! description = describe span
    let! dims =
        match description.Shape, description.Placement, description.Gates with
        | File None, None, [] -> Ok None
        | File None, Some (Dims dims), [] -> Ok (Some dims)
        | _ -> at span (BindingDescription (text span))
    return { Type = description.Type; Dims = dims }
}

let readProduct e = reading {
    let! names, declared, callText = reading {
        match e.Content with
        | Content.Bare ->
            let! names, call = separated "=" e.Head
            return names, None, call
        | Content.Value value ->
            let! description, call = separated "=" value
            let! declared = readBindingDescription description
            return e.Head, Some declared, call
        | _ -> return! at e.Head (BindingDescription (contentText e))
    }
    let! callee, args = applied callText
    let! args = readList readArg args
    let! callee = readRef callee
    let call = { Callee = callee; Args = args }
    let! names = commaList names |> Result.bind (readList (checkName "`product`"))
    return { Names = names; Call = call; Declared = declared }
}

let readUse e = reading {
    let! alias =
        if empty e.Head then Ok None
        else checkName "`use`" e.Head |> Result.map Some
    let! source =
        match e.Content with
        | Content.Value value -> Ok value
        | _ -> at e.Header.Rest (MissingImportSource (text e.Header.Rest))
    let! tokens = words source
    match tokens with
    | [ path ] -> return { Names = None; From = text path; Alias = alias }
    | [ path; names ] ->
        let! names = enclosed '[' ']' names
        let! names = readList (checkName "an imported name") names
        return { Names = Some names; From = text path; Alias = alias }
    | _ -> return! at source (InvalidImport (text source))
}

let readTypeParameter input = reading {
    let! value, rest = readTypePrefix 0 input
    match value with
    | Var name -> return name, rest
    | other -> return! at (trim (slice input.Start rest.Start input)) (TypeParameterNeedsVariable other)
}

let readTypeDef input = reading {
    if starts "$" input then
        let! value = readType input
        match value with
        | Var name -> return! at input (DefinedTypeVariable name)
        | Named (name, _) -> return! at input (InvalidTypeName name)
    else
        let! nameSpan, rest = namePrefix input
        let! name = typeName nameSpan
        let! parameters, rest =
            if starts "<" rest then commaItems (Some ">") readTypeParameter (drop 1 rest)
            else Ok ([], rest)
        do! requireEnd rest
        return { Name = name; Params = parameters }
}

let readDefaultGate kind e = reading {
    let read span = reading {
        let! useGate = readGateUse span
        if useGate.Negated then return! at span (NegatedDefaultGate (kind, useGate.Gate.Name))
        else return useGate
    }
    let! span = single e
    let! uses = commaList span |> Result.bind (readList read)
    return Setting.Gate (kind, EveryArtifact, uses)
}

let readReview name parameters e =
    match parameters, e.Content with
    | _ :: _, _ -> at e.Head (ReviewParameters name)
    | [], Content.Bare -> Ok (GateDef.Review name)
    | [], Content.Value value -> at value (ReviewScript (name, text value))
    | [], _ -> at e.Head (ReviewScript (name, contentText e))

let readTest constructor kind name parameters e =
    let script = scriptLines e
    if script.IsEmpty then at e.Head (GateNeedsScript (kind, name, text e.Head))
    else Ok (constructor { Name = name; Params = parameters; Script = script })

let readGateDefinition kind e = reading {
    let! name, parameters = applied e.Head
    let! name = checkName (owner e) name
    let! parameters = readList (checkName "a parameter") parameters
    match kind with
    | GateKind.Review -> return! readReview name parameters e
    | GateKind.Check -> return! readTest GateDef.Check kind name parameters e
    | GateKind.Screen -> return! readTest GateDef.Screen kind name parameters e
}

let valueOnly e =
    match empty e.Head, e.Content with
    | true, Content.Value value -> Ok value
    | _ -> at e.Head (ExpectedValueOnly (e.Header.Rule.Word, text e.Header.Rest))

let nameOnly e =
    match empty e.Head, e.Content with
    | false, Content.Bare -> Ok e.Head
    | false, Content.Value value -> at value (ExpectedNameOnly (e.Header.Rule.Word, text value))
    | _ -> at e.Head (ExpectedName e.Header.Rule.Word)

let scopeOf (items: Item list) : Scope =
    { Operations = items |> List.choose (function Item.Operation o -> Some o | _ -> None)
      Artifacts = items |> List.choose (function Item.Artifact a -> Some a | _ -> None)
      Settings = items |> List.choose (function Item.Setting s -> Some s | _ -> None)
      Stages = items |> List.choose (function Item.Stage s -> Some s | _ -> None) }

let validateContext nested e =
    if e.Header.Default && not e.Header.Rule.AllowsDefault then at e.Head (DisallowedDefault e.Header.Rule.Keyword)
    elif nested && e.Header.Rule.TopLevel && not e.Header.Default then at e.Head NestedDefinition
    else Ok ()

let readResources span =
    commaList span |> Result.bind (readList (separated "="))
    |> Result.map (List.map (fun (name, value) -> text name, text value))

let readValueSetting readTarget parseValue constructor e = reading {
    let! target = readTarget e
    let! value = single e |> Result.bind parseValue
    return Ready (Item.Setting (constructor (target, value)))
}

let readScriptSetting constructor e = reading {
    let! target = operationTarget e
    return Ready (Item.Setting (constructor (target, scriptLines e)))
}

let readAnchored e = reading {
    let! target =
        if e.Header.Default then operationTarget e
        else nameOnly e |> Result.bind (checkName (owner e)) |> Result.map NamedOperation
    return Ready (Item.Setting (Setting.Anchored target))
}

let readGate kind e =
    if e.Header.Default then readDefaultGate kind e |> Result.map (Item.Setting >> Ready)
    else readGateDefinition kind e |> Result.map (Item.GateDef >> Ready)

let rec readScope depth pending =
    let read entry = reading {
        let! entry = entry
        do! validateContext (depth > 0) entry
        return! readStatement depth entry
    }
    { Entries = entries None false pending |> List.map read }

and readStatement depth e =
    match e.Header.Rule.Keyword with
    | Keyword.Types -> valueOnly e |> Result.map (fun value -> Ready (Item.Types { Library = text value }))
    | Keyword.Type -> nameOnly e |> Result.bind readTypeDef |> Result.map (Item.TypeDef >> Ready)
    | Keyword.Use -> readUse e |> Result.map (Item.Import >> Ready)
    | Keyword.Root -> valueOnly e |> Result.map (text >> Item.Root >> Ready)
    | Keyword.Output -> valueOnly e |> Result.map (text >> Item.OutputDir >> Ready)
    | Keyword.Dimensions -> valueOnly e |> Result.bind (enclosed '[' ']') |> Result.bind (readList (checkName "a dimension")) |> Result.map (Item.Dimensions >> Ready)
    | Keyword.Tool -> nameOnly e |> Result.bind (checkName (owner e)) |> Result.map (Item.Tool >> Ready)
    | Keyword.Source -> readSource e |> Result.map SourceDeclaration
    | Keyword.Product -> readProduct e |> Result.map ProductBinding
    | Keyword.Check -> readGate GateKind.Check e
    | Keyword.Screen -> readGate GateKind.Screen e
    | Keyword.Review -> readGate GateKind.Review e
    | Keyword.Path -> readValueSetting artifactTarget (text >> Ok) Setting.Path e
    | Keyword.Ext -> readValueSetting artifactTarget (text >> Ok) Setting.Ext e
    | Keyword.Entities -> readValueSetting dimensionTarget (text >> Ok) Setting.Entities e
    | Keyword.Command -> readScriptSetting Setting.Command e
    | Keyword.Verify -> readScriptSetting Setting.Verify e
    | Keyword.Calculate -> readValueSetting operationTarget (text >> Ok) Setting.Calculate e
    | Keyword.Resources -> readValueSetting operationTarget readResources Setting.Resources e
    | Keyword.Uses -> readValueSetting operationTarget (commaList >> Result.bind (readList readToolUse)) Setting.Uses e
    | Keyword.Anchored -> readAnchored e
    | Keyword.Stage -> readStage depth e
    | Keyword.Operation -> readOperationDeclaration depth e

and readStage depth e = reading {
    let! name = checkName (owner e) e.Head
    let! body = readBody depth e
    match body with
    | Some scope -> return StageDeclaration (name, scope)
    | None -> return! at e.Head ExpectedStageBody
}

and readOperationDeclaration depth e = reading {
    let! operation = readOperation e.Head
    let! body = readBody depth e
    return OperationDeclaration { operation with Body = body }
}

and readBody depth e =
    match e.Content with
    | Content.Body lines when depth >= maxNesting -> at e.Head NestingLimit
    | Content.Body lines -> Ok (Some (readScope (depth + 1) lines))
    | Content.Bare -> Ok None
    | _ -> at e.Head (UnexpectedBodyValue (text e.Head, contentText e))

let gatesOn target annotations =
    annotations
    |> List.map (fun annotation -> Item.Setting (Setting.Gate (annotation.Kind, target, annotation.Uses)))

let rec lowerScope scope =
    let results = scope.Entries |> List.map (function Error diagnostic -> [], [ diagnostic ] | Ok statement -> lowerStatement statement)
    List.collect fst results, List.collect snd results

and lowerStatement = function
    | Ready item -> [ item ], []
    | SourceDeclaration source -> Item.Artifact source.Value :: gatesOn (NamedArtifact source.Value.Name) source.Gates, []
    | ProductBinding binding ->
        binding.Names |> List.mapi (fun i name ->
            Item.Artifact { Name = name; Origin = Made (binding.Call, i, binding.Declared) }), []
    | StageDeclaration (name, body) ->
        let items, diagnostics = lowerScope body
        [ Item.Stage { Name = name; Scope = scopeOf items } ], diagnostics
    | OperationDeclaration operation ->
        let body, diagnostics =
            match operation.Body with
            | None -> None, []
            | Some body ->
                let items, diagnostics = lowerScope body
                Some (scopeOf items), diagnostics
        let declaration: OperationDecl =
            { Name = operation.Name; Params = List.map _.Value operation.Params
              Outputs = List.map _.Value operation.Outputs; Body = body }
        let inputs = operation.Params |> List.collect (fun input -> gatesOn (InputOf (operation.Name, input.Value.Name)) input.Gates)
        let outputs = operation.Outputs |> List.collect (fun output -> gatesOn (OutputOf (operation.Name, output.Value.Name)) output.Gates)
        Item.Operation declaration :: inputs @ outputs, diagnostics

let pipelineOf (items: Item list) : Pipeline =
    { Types = items |> List.tryPick (function Item.Types t -> Some t | _ -> None)
      Root = items |> List.tryPick (function Item.Root r -> Some r | _ -> None)
      OutputDir = items |> List.tryPick (function Item.OutputDir o -> Some o | _ -> None)
      Imports = items |> List.choose (function Item.Import u -> Some u | _ -> None)
      TypeDefs = items |> List.choose (function Item.TypeDef t -> Some t | _ -> None)
      Dimensions = items |> List.collect (function Item.Dimensions d -> d | _ -> [])
      Tools = items |> List.choose (function Item.Tool t -> Some t | _ -> None)
      Gates = items |> List.choose (function Item.GateDef g -> Some g | _ -> None)
      Global = scopeOf items }

let parse (text: string) : Pipeline * Diagnostic list =
    let items, diagnostics = text |> lines |> readScope 0 |> lowerScope
    pipelineOf items, diagnostics

let show value = sprintf "%0A" value

let rec printScope (indent: string) (scope: Scope) =
    for o in scope.Operations do
        printfn "%soperation %s" indent (show { o with Body = None })
        o.Body |> Option.iter (printScope (indent + "    "))
    for a in scope.Artifacts do printfn "%sartifact %s" indent (show a)
    for s in scope.Settings do printfn "%ssetting %s" indent (show s)
    for st in scope.Stages do
        printfn "%sstage %s" indent st.Name
        printScope (indent + "    ") st.Scope

let printPipeline (pipeline: Pipeline, diagnostics: Diagnostic list) =
    printfn "types %s" (show pipeline.Types)
    printfn "root %s, output %s" (show pipeline.Root) (show pipeline.OutputDir)
    for u in pipeline.Imports do printfn "import %s" (show u)
    for t in pipeline.TypeDefs do printfn "type %s" (show t)
    printfn "dimensions %s" (show pipeline.Dimensions)
    printfn "tools %s" (show pipeline.Tools)
    for g in pipeline.Gates do printfn "gate %s" (show g)
    printfn "global"
    printScope "    " pipeline.Global
    if diagnostics.IsEmpty then printfn "no diagnostics"
    for d in diagnostics do printfn "line %d: %s" (diagnosticLine d) (renderProblem gateWord d.Problem)

let run files =
    for file in files do
        printfn "=== %s" file
        System.IO.File.ReadAllText (System.IO.Path.Combine (__SOURCE_DIRECTORY__, "../../ocaml/examples", file)) |> parse |> printPipeline
        printfn ""

if System.IO.Path.GetFullPath fsi.CommandLineArgs.[0] = System.IO.Path.Combine (__SOURCE_DIRECTORY__, "mock_parser.fsx") then
    let files =
        match fsi.CommandLineArgs |> Array.toList |> List.tail with
        | [] -> [ "v1_mock.spit"; "v2_mock.spit"; "v3_mock.spit"; "v4_mock.spit"; "stats_mock.spit" ]
        | given -> given
    run files
