open Ast
open Data

type tool = { name : string; identity : string option; probe : string list option }

let probe text =
  let span = { source = text; start = 0; finish = String.length text } in
  let* script = Inventory.syntax "tool" (Template.script ~indent:0 span) in
  match script.commands with
  | [ command ] ->
      read_list
        (fun (word : command_word) ->
          if word.role <> Argument then error Problem.Probe_redirect
          else
            let+ parts =
              read_list
                (function Text text -> Ok text.value | Expand _ -> error Problem.Probe_literal)
                word.parts
            in
            String.concat "" parts)
        command.words
  | _ -> error Problem.Probe_one_command

let entry text =
  match String.index_opt text ':' with
  | None -> error Problem.Tool_entry
  | Some at ->
      let name = String.trim (String.sub text 0 at) in
      let text = String.trim (String.sub text (at + 1) (String.length text - at - 1)) in
      if String.starts_with ~prefix:"probe " text then
        let+ probe = probe (String.sub text 6 (String.length text - 6)) in
        { name; identity = None; probe = Some probe }
      else if text = "" then error (Problem.Tool_empty name)
      else Ok { name; identity = Some text; probe = None }

let parse text =
  let read (seen, tools) (line : line) =
    if Layout.ignored line then Ok (seen, tools)
    else
      let text = Span.text (Span.trim line.span) in
      if line.indent = 0 && text = "tools:" && not seen then Ok (true, tools)
      else if (not seen) || line.indent = 0 then error Problem.Tools_section
      else
        let* tool = entry text in
        if List.exists (fun previous -> previous.name = tool.name) tools then
          error (Problem.Duplicate_tool tool.name)
        else Ok (seen, tool :: tools)
  in
  let+ _, tools =
    List.fold_left
      (fun state line ->
        let* state = state in
        read state line)
      (Ok (false, []))
      (Layout.lines text)
  in
  List.rev tools

let json tool =
  Json.Object
    [
      ("name", Json.Text tool.name);
      ("identity", Json.option Json.str tool.identity);
      ("probe", Json.option Json.strings tool.probe);
    ]

let used declared operation settings =
  List.fold_left
    (fun tools (setting : Setting.t located) ->
      match setting.value with
      | Uses (target, uses) when target = Every_operation || target = Named_operation operation ->
          List.fold_left
            (fun tools used ->
              if used.used && List.mem used.tool declared && not (List.mem used.tool tools) then
                tools @ [ used.tool ]
              else if not used.used then List.filter (( <> ) used.tool) tools
              else tools)
            tools uses
      | _ -> tools)
    [] settings

let validate declared tools =
  let names = List.map (fun tool -> tool.name) tools in
  match List.find_opt (fun name -> not (List.mem name names)) declared with
  | Some name -> error (Problem.Tool_without_identity name)
  | None -> Ok ()
