module MockModel

type Keyword =
    | Types | Type | Use | Root | Output | Dimensions | Source | Path
    | Check | Screen | Review | Tool | Command | Verify | Calculate
    | Resources | Uses | Anchored | Stage | Product | Operation | Entities | Ext

type TypeExpr =
    | Named of name: string * args: TypeExpr list
    | Var of string

type TypeDef = { Name: string; Params: string list }

type TypesDecl = { Library: string }

type UseDecl = { Names: string list option; From: string; Alias: string option }

type Ref = { Namespace: string option; Name: string }

[<RequireQualifiedAccess>]
type GateKind =
    | Check
    | Screen
    | Review

type Test = { Name: string; Params: string list; Script: string list }

[<RequireQualifiedAccess>]
type GateDef =
    | Check of Test
    | Screen of Test
    | Review of name: string

type GateUse = { Gate: Ref; Args: string list; Negated: bool }

type Beside = { Of: string; Suffix: string option }

type Placement =
    | Dims of string list
    | Beside of Beside
    | Here

type Ext =
    | Replace of string
    | Append of string

type Shape =
    | File of Ext option
    | Folder
    | FolderFrom of param: string
    | View

type Constraint = Min of int

type Param =
    { Name: string
      Many: bool
      Type: TypeExpr option
      Constraints: Constraint list }

type Output =
    { Name: string option
      Type: TypeExpr option
      Shape: Shape
      Beside: Beside option }

type Iteration =
    | Vary of string list
    | Each of string
    | Same of string list
    | Where of (string * string) list

type Arg = { Value: string; Iteration: Iteration option }

type Call = { Callee: Ref; Args: Arg list }

type Declared = { Type: TypeExpr option; Dims: string list option }

type Found =
    { Type: TypeExpr option
      Ext: Ext option
      Placement: Placement }

type Origin =
    | Found of Found
    | Made of call: Call * output: int * declared: Declared option

type Artifact = { Name: string; Origin: Origin }

type OperationTarget =
    | EveryOperation
    | NamedOperation of string

type ArtifactTarget =
    | EveryArtifact
    | NamedArtifact of string
    | OutputOf of operation: string * output: string option
    | InputOf of operation: string * param: string

type DimensionTarget =
    | EveryDimension
    | NamedDimension of string

type ToolUse = { Tool: string; Used: bool }

[<RequireQualifiedAccess>]
type Setting =
    | Command of OperationTarget * lines: string list
    | Verify of OperationTarget * lines: string list
    | Calculate of OperationTarget * expression: string
    | Resources of OperationTarget * (string * string) list
    | Uses of OperationTarget * ToolUse list
    | Anchored of OperationTarget
    | Path of ArtifactTarget * template: string
    | Gate of GateKind * ArtifactTarget * GateUse list
    | Ext of ArtifactTarget * string
    | Entities of DimensionTarget * spelling: string

type OperationDecl =
    { Name: string
      Params: Param list
      Outputs: Output list
      Body: Scope option }

and Scope =
    { Operations: OperationDecl list
      Artifacts: Artifact list
      Settings: Setting list
      Stages: Stage list }

and Stage = { Name: string; Scope: Scope }

type Pipeline =
    { Types: TypesDecl option
      Root: string option
      OutputDir: string option
      Imports: UseDecl list
      TypeDefs: TypeDef list
      Dimensions: string list
      Tools: string list
      Gates: GateDef list
      Global: Scope }

type EntryForm =
    | Plain
    | Script
    | Gate of GateKind

type KeywordRule =
    { Keyword: Keyword
      Word: string
      Form: EntryForm
      AllowsDefault: bool
      TopLevel: bool }

type SourceSpan = { Source: string; Start: int; Finish: int }

type GateAnnotation = { Span: SourceSpan; Kind: GateKind; Uses: GateUse list }

type Line = { Indent: int; Span: SourceSpan }

type EntryHeader = { Rule: KeywordRule; Default: bool; Rest: SourceSpan }

[<RequireQualifiedAccess>]
type Content =
    | Bare
    | Value of SourceSpan
    | Script of SourceSpan * indent: int
    | Body of Line list

type Entry = { Header: EntryHeader; Head: SourceSpan; Content: Content }

type Description =
    { Type: TypeExpr option
      Shape: Shape
      Placement: Placement option
      Gates: GateAnnotation list }

[<RequireQualifiedAccess>]
type Item =
    | Types of TypesDecl
    | Root of string
    | OutputDir of string
    | Import of UseDecl
    | TypeDef of TypeDef
    | Dimensions of string list
    | Tool of string
    | GateDef of GateDef
    | Operation of OperationDecl
    | Artifact of Artifact
    | Setting of Setting
    | Stage of Stage

let keywords =
    let definition keyword word form =
        { Keyword = keyword; Word = word; Form = form; AllowsDefault = false; TopLevel = true }
    let scoped keyword word form allowsDefault =
        { Keyword = keyword; Word = word; Form = form; AllowsDefault = allowsDefault; TopLevel = false }
    let gate keyword word kind =
        { definition keyword word (Gate kind) with AllowsDefault = true }
    [ definition Keyword.Types "types" Plain
      definition Keyword.Type "type" Plain
      definition Keyword.Use "use" Plain
      definition Keyword.Root "root" Plain
      definition Keyword.Output "output" Plain
      definition Keyword.Dimensions "dimensions" Plain
      definition Keyword.Tool "tool" Plain
      gate Keyword.Check "check" GateKind.Check
      gate Keyword.Screen "screen" GateKind.Screen
      gate Keyword.Review "review" GateKind.Review
      scoped Keyword.Source "source" Plain false
      scoped Keyword.Path "path" Plain true
      scoped Keyword.Command "command" Script true
      scoped Keyword.Verify "verify" Script true
      scoped Keyword.Calculate "calculate" Plain true
      scoped Keyword.Resources "resources" Plain true
      scoped Keyword.Uses "uses" Plain true
      scoped Keyword.Anchored "anchored" Plain true
      scoped Keyword.Stage "stage" Plain true
      scoped Keyword.Product "product" Plain false
      scoped Keyword.Operation "operation" Plain false
      scoped Keyword.Entities "entities" Plain true
      scoped Keyword.Ext "ext" Plain true ]
    |> List.map (fun rule -> rule.Word, rule)
    |> Map.ofList

let gateWord kind =
    keywords
    |> Map.pick (fun word rule -> if rule.Form = Gate kind then Some word else None)

type Problem =
    | UnclosedQuote of char
    | UnexpectedClosing of char
    | EmptyListItem
    | NestingLimit
    | UnexpectedText of string
    | UnclosedBracket
    | InlineComment
    | MissingBindingKeyword of string
    | UnknownKeyword of string
    | StageNeedsName
    | InlineScriptWithBody of head: string * line: string
    | MissingScript of word: string * head: string
    | UnexpectedIndent of string
    | ValueCount of head: string * count: int
    | MissingName of owner: string
    | DefaultUsedAsName
    | ReservedName of string
    | InvalidName of string
    | NamedDefault of string
    | DefaultTarget of word: string * value: string
    | MissingSeparator of separator: string * text: string
    | MissingDelimiters of opening: char * closing: char * text: string
    | FolderExtension
    | DescriptionOrder of token: string
    | EmptyGateAnnotation of GateKind
    | RepeatedGateAnnotation of GateKind
    | ContradictedGate of string
    | FolderSource of string
    | OutputPlacement
    | InvalidMinimum of string
    | InvalidParameterAnnotation of string
    | InvalidArgumentAnnotation of string
    | InvalidArgument of string
    | BindingDescription of string
    | ExpectedValueOnly of word: string * rest: string
    | ExpectedNameOnly of word: string * value: string
    | ExpectedName of word: string
    | MissingImportSource of string
    | InvalidImport of string
    | TypeParameterNeedsVariable of TypeExpr
    | InvalidTypeName of string
    | DefinedTypeVariable of string
    | NegatedDefaultGate of GateKind * name: string
    | GateNeedsScript of GateKind * name: string * head: string
    | ReviewParameters of string
    | ReviewScript of name: string * value: string
    | DisallowedDefault of Keyword
    | ExpectedStageBody
    | UnexpectedBodyValue of head: string * value: string
    | NestedDefinition

type Diagnostic = { Span: SourceSpan; Problem: Problem }


type Annotated<'T> = { Value: 'T; Gates: GateAnnotation list }

type Binding = { Names: string list; Call: Call; Declared: Declared option }

type ParsedOperation =
    { Name: string
      Params: Annotated<Param> list
      Outputs: Annotated<Output> list
      Body: ParsedScope option }

and ParsedScope = { Entries: Result<Statement, Diagnostic> list }

and Statement =
    | Ready of Item
    | SourceDeclaration of Annotated<Artifact>
    | OperationDeclaration of ParsedOperation
    | ProductBinding of Binding
    | StageDeclaration of name: string * ParsedScope
