open Plan_support

let blob_ids =
  List.filter_map
    (fun (text, expected) ->
      let found = Blob.id text in
      if found = expected then None
      else
        Some
          (Printf.sprintf "blob of %d bytes is %s, expected %s" (String.length text) found expected))
    [
      ("", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
      (String.make 55 'a', "d1985ddc2983785702b9a90effd5aff2f7cfdca4");
      (String.make 56 'a', "1f973e890f52da1f22fa7e5620a628bc4ee74cb3");
      (String.make 64 'a', "71b7a71962774fa5c721e1163f935cb61a0e09e6");
      (String.make 119 'a', "1325c3d76ea6a66e75dce490a0b8cb8b12233ee0");
      ("hello\n", "ce013625030ba8dba906f756967f9e9ca394464a");
    ]

let tests =
  [
    test "pipeline_files_name_each_file_with_its_blob_id"
      ~sources:[ source "raw" ~typ:"Mean" [ "subject" ] ]
      (operation "copy(x: Mean) -> Mean" ^ "copied = copy(raw)\n")
      ~data:[ "raw[subject=a]" ]
      [ plan [ at "pipeline_files.#" "1"; at "pipeline_files.0.path" {|"pipeline.spit"|} ] ];
  ]
