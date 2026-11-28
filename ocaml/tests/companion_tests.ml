open Plan_support
open Problem

let entities = "default entities: {@key}={@value} separated \"_\"\n"

let dwi =
  "source raw_dwi: Dwi .nii.gz [sub, ses, run]\n\
   path raw_dwi: sub-{sub}/ses-{ses}/dwi/sub-{sub}_ses-{ses}_run-{run}_dwi\n\
   source reverse_b0: B0 .nii.gz [sub, ses]\n\
   path reverse_b0: sub-{sub}/ses-{ses}/fmap/sub-{sub}_ses-{ses}_epi\n\
   stage sidecars:\n\
  \    default path: {raw_dwi$dir}/{raw_dwi$stem}\n\
  \    source dwi_bvec: Bvec .bvec\n\
  \    source dwi_bval: Bval .bval\n\
  \    source dwi_json: Metadata .json\n"

let preprocess =
  "operation fit(dwi: Dwi, bvec: Bvec, bval: Bval, json: Metadata, b0: B0) -> Model\n\
   command fit: fit {dwi} {bvec} {bval} {json} {b0} {@output}\n\
   model = fit(raw_dwi, dwi_bvec, dwi_bval, dwi_json, reverse_b0)\n"

let scan = "sub-01/ses-1/dwi/sub-01_ses-1_run-1_dwi"

let dwi_files =
  [
    scan ^ ".nii.gz";
    scan ^ ".bvec";
    scan ^ ".bval";
    scan ^ ".json";
    "sub-01/ses-1/fmap/sub-01_ses-1_epi.nii.gz";
  ]

let sidecars =
  test "sidecars_in_a_stage_take_their_paths_from_the_source_they_sit_beside"
    ~header:("default path: out/{@product}/{@entities}\n" ^ entities)
    ~files:dwi_files (dwi ^ preprocess)
    [
      plan
        [
          jobs 1;
          at "jobs.0.inputs.bvec.0.path" ("\"" ^ scan ^ ".bvec\"");
          at "jobs.0.inputs.bval.0.path" ("\"" ^ scan ^ ".bval\"");
          at "jobs.0.inputs.json.0.path" ("\"" ^ scan ^ ".json\"");
          at "jobs.0.inputs.json.0.entities" {|{"sub":"01","ses":"1","run":"1"}|};
          at "jobs.0.inputs.b0.0.path" {|"sub-01/ses-1/fmap/sub-01_ses-1_epi.nii.gz"|};
        ];
    ]

let note =
  test "a_source_can_take_its_path_from_another_source" ~header:entities
    ~sources:
      [ source "location" ~typ:"Site" ~path:"{region}/{site}/site.json" [ "region"; "site" ] ]
    ~files:[ "coast/dune/site.json"; "coast/dune/site.txt" ]
    ("source location_note: Text .txt\npath location_note: {location$dir}/{location$stem}.txt\n"
   ^ "operation card(location: Site, note: Text) -> Text\n\
      command card: cat {note} {location} > {@output}\n\
      cards = card(location, location_note)\n\
      path cards: cards/{region}/{site}.txt\n")
    [
      plan
        [
          at "jobs.0.inputs.note.0.path" {|"coast/dune/site.txt"|};
          at "jobs.0.inputs.note.0.entities" {|{"region":"coast","site":"dune"}|};
        ];
    ]

let report =
  "operation write(board: Table) -> (report: Text .txt, summary: Text .txt)\n\
   command write: report {board} {report} {summary}\n\
   written, written_summary = write(board)\n\
   path written: report/{site}.txt\n\
   path written_summary: {written$dir}/{written$stem}_summary.txt\n"

let tests =
  [
    sidecars;
    vary "a_missing_sidecar_names_its_path" ~data:[]
      [ plan [ problem (Companion_missing { product = "dwi_json"; path = scan ^ ".json" }) ] ]
      {
        sidecars with
        files = List.filter (fun f -> not (String.ends_with ~suffix:".json" f)) dwi_files;
      };
    note;
    test "an_output_can_take_its_path_from_another_output" ~header:entities
      ~sources:[ source "board" ~typ:"Table" ~path:"in/{site}.tsv" [ "site" ] ]
      ~files:[ "in/dune.tsv" ] report
      [
        plan
          [
            at "jobs.0.outputs.report.path" {|"report/dune.txt"|};
            at "jobs.0.outputs.summary.path" {|"report/dune_summary.txt"|};
          ];
      ];
    test "an_output_can_sit_beside_its_input" ~header:entities
      ~sources:[ source "series" ~typ:"Readings" ~path:"in/{site}.csv" [ "site" ] ]
      ~files:[ "in/dune.csv" ]
      "operation index(series: Readings) -> Index .idx\n\
       command index: sh -c 'wc -l < \"$1\" > \"$1.idx\"' sh {series}\n\
       indexed = index(series)\n\
       path indexed: {series$dir}/{series$stem}.csv.idx\n"
      [ plan [ at "jobs.0.outputs.output.path" {|"in/dune.csv.idx"|} ] ];
    test "a_path_property_is_dir_or_stem" ~header:entities
      ~sources:[ source "a" ~path:"in/{id}.txt" [ "id" ]; source "b" ~path:"{a$name}.txt" [ "id" ] ]
      ""
      [ check [ parse (Path_property_unknown "a$name") ] ];
    test "a_reference_names_a_product" ~header:entities
      ~sources:[ source "b" ~path:"{missing$dir}/b.txt" [ "id" ] ]
      ""
      [ check [ problem (Reference_unknown { product = "b"; reference = "missing" }) ] ];
    test "a_rule_cannot_refer_to_its_own_product" ~header:entities
      ~sources:[ source "b" ~path:"{b$dir}/{id}.txt" [ "id" ] ]
      ""
      [ check [ problem (Reference_self "b") ] ];
    test "a_source_cannot_take_its_path_from_an_output" ~header:entities
      ~sources:
        [
          source "raw" ~path:"in/{id}.txt" [ "id" ];
          source "late" ~path:"{copied$dir}/late.txt" [ "id" ];
        ]
      "operation copy(x) ->\n\
       command copy: cp {x} {@output}\n\
       copied = copy(raw)\n\
       path copied: out/{id}.txt\n"
      [ check [ problem (Reference_not_source { product = "late"; reference = "copied" }) ] ];
    test "a_reference_has_the_same_dimensions" ~header:entities
      ~sources:
        [
          source "a" ~path:"in/{id}.txt" [ "id" ];
          source "b" ~path:"{a$dir}/{a$stem}_{run}.txt" [ "id"; "run" ];
        ]
      ""
      [ check [ problem (Reference_dimensions { product = "b"; reference = "a" }) ] ];
    test "a_reference_is_not_a_chain" ~header:entities
      ~sources:
        [
          source "a" ~path:"in/{id}.txt" [ "id" ];
          source "b" ~path:"{a$dir}/{a$stem}.b" [ "id" ];
          source "c" ~path:"{b$dir}/{b$stem}.c" [ "id" ];
        ]
      ""
      [ check [ problem (Reference_chain { product = "c"; reference = "b"; next = "a" }) ] ];
  ]
