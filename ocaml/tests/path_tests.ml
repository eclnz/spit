open Plan_support
open Problem

let entities = "default entities: {@key}={@value} separated \"__\"\n"

let out = "default path: out/{@product}/{@entities}\n" ^ entities

let raw = source "raw" ~typ:"Image" ~path:"in/{id}.raw" [ "id" ]

let steps =
  operation "copy(input: Image) -> Image"
  ^ operation "align(input: Image) -> Transform .mat"
  ^ "operation fit(input: Image) -> (weights: Weights .npz, quality: Metrics .tar.gz)\n\
     command fit: fit {input} {weights} {quality}\n"
  ^ operation "untyped(input) -> .txt"
  ^ "copied = copy(raw)\nmatrix = align(raw)\nweights, quality = fit(raw)\nnote = untyped(raw)\n"

let planned ?gap ?(header = out) name ?(sources = [ raw ]) ?(files = [ "in/a.raw" ]) steps expects =
  test name ?gap ~header ~sources ~files steps [ plan expects ]

let checked ?gap ?(header = out) name ?(sources = [ raw ]) steps expects =
  test name ?gap ~header ~sources steps [ check expects ]

let path job port = Printf.sprintf "jobs.%d.outputs.%s.path" job port

let quoted text = "\"" ^ text ^ "\""

let extensions =
  [
    planned "an_operation_completes_the_default_path_with_its_outputs_extensions" steps
      [
        at (path 0 "output") (quoted "out/copied/id=a");
        at (path 1 "output") (quoted "out/matrix/id=a.mat");
        at (path 2 "weights") (quoted "out/weights/id=a.npz");
        at (path 2 "quality") (quoted "out/quality/id=a.tar.gz");
        at (path 3 "output") (quoted "out/note/id=a.txt");
        at "external_inputs.0.path" (quoted "in/a.raw");
      ];
    planned "default_ext_completes_the_default_for_operations_that_declare_none"
      ~header:(out ^ "default ext: .img\n") steps
      [
        at (path 0 "output") (quoted "out/copied/id=a.img");
        at (path 1 "output") (quoted "out/matrix/id=a.mat");
      ];
    planned "a_stage_ext_applies_to_its_steps_and_nested_stages"
      ~header:(out ^ "default ext: .mif\n")
      (operation "copy(input: Image) -> Image"
      ^ operation "align(input: Image) -> Transform .mat"
      ^ "outside = copy(raw)\n\
         stage anatomy:\n\
        \    default ext: .nii.gz\n\
        \    inside = copy(raw)\n\
        \    stage deeper:\n\
        \        nested = copy(raw)\n\
        \        matrix = align(raw)\n")
      [
        at "jobs.*.outputs.output.path"
          {|["out/outside/id=a.mif","out/inside/id=a.nii.gz","out/nested/id=a.nii.gz","out/matrix/id=a.mat"]|};
      ];
    planned "a_products_own_rule_takes_only_its_operations_extension"
      ~header:(out ^ "default ext: .img\n")
      (steps
     ^ "path matrix: transforms/{id}\npath weights: fits/{id}.npz\npath copied: copies/{id}\n")
      [
        at (path 0 "output") (quoted "copies/a");
        at (path 1 "output") (quoted "transforms/a.mat");
        at (path 2 "weights") (quoted "fits/a.npz");
      ];
    planned "a_source_on_the_default_rule_takes_default_ext"
      ~header:("default path: data/{@product}/{@entities}\ndefault ext: .csv\n" ^ entities)
      ~sources:[ source "table" ~path:"" [ "id" ] ]
      ~files:[ "data/table/id=a.csv" ] (reads [ "table" ])
      [ at "external_inputs.0.path" (quoted "data/table/id=a.csv") ];
    planned "a_source_declares_its_extension_as_an_output_does"
      ~header:("default path: data/{@product}/{@entities}\ndefault ext: .csv\n" ^ entities)
      ~sources:[]
      ~files:[ "data/image/id=a.nii.gz"; "logs/a.tsv"; "data/table/id=a.csv" ]
      ("source image: Image .nii.gz [id]\n\
        source events: .tsv [id]\n\
        source table: [id]\n\
        path events: logs/{id}\n"
      ^ reads [ "image"; "events"; "table" ])
      [
        at "external_inputs.*.path"
          {|["data/image/id=a.nii.gz","data/table/id=a.csv","logs/a.tsv"]|};
      ];
    checked "a_sources_rule_ending_in_another_extension_is_an_error" ~sources:[]
      ("source events: .tsv [id]\npath events: logs/{id}.csv\n" ^ reads [ "events" ])
      [
        problem
          (Rule_extension
             {
               product = "events";
               literal = ".csv";
               writer = Source_declares "events";
               ext = ".tsv";
             });
      ];
    checked "a_default_path_ending_in_another_extension_than_a_sources_is_an_error"
      ~header:("default path: in/{@product}/{@entities}.csv\n" ^ entities)
      ~sources:[]
      ("source events: .tsv [id]\n" ^ reads [ "events" ])
      [
        problem
          (Default_extension_source
             {
               product = "events";
               literal = ".csv";
               writer = Source_declares "events";
               ext = ".tsv";
             });
      ];
    checked "an_extension_needs_a_dot_and_name_characters" ~sources:[] "source raw: .c$v [id]\n"
      [ parse (Invalid_extension ".c$v") ];
    checked "a_rule_ending_in_another_extension_is_an_error"
      (steps ^ "path matrix: transforms/{id}.txt\n")
      [
        problem
          (Rule_extension
             {
               product = "matrix";
               literal = ".txt";
               writer = Operation_writes "align";
               ext = ".mat";
             });
      ];
    planned "a_dot_earlier_in_the_file_name_is_not_part_of_its_extension"
      (steps ^ "path matrix: transforms/{id}_acq-1.5T.mat\n")
      [ at (path 1 "output") (quoted "transforms/a_acq-1.5T.mat") ];
    checked "a_default_rule_ending_in_another_extension_is_one_error"
      ~header:("default path: out/{@product}/{@entities}.img\n" ^ entities)
      (steps ^ "also = align(raw)\n")
      [
        problem
          (Default_extension_output
             {
               product = "matrix";
               literal = ".img";
               writer = Operation_writes "align";
               ext = ".mat";
             });
      ];
    checked "a_stage_default_path_and_its_default_ext_must_agree" ~header:entities
      (operation "copy(input: Image) -> Image"
      ^ "stage s:\n\
        \    default path: out/{@product}/{@entities}.img\n\
        \    default ext: .txt\n\
        \    copied = copy(raw)\n")
      [ problem (Default_extension_setting { product = "copied"; literal = ".img"; ext = ".txt" }) ];
    checked "a_default_path_and_default_ext_that_agree_are_fine"
      ~header:("default path: out/{@product}/{@entities}.img\ndefault ext: .img\n" ^ entities)
      (operation "copy(input: Image) -> Image" ^ "copied = copy(raw)\n")
      [];
    checked "an_output_extension_is_checked_when_parsed" "operation f(x) -> Image mat.\n"
      [ parse (Description_order "mat.") ];
    checked "a_default_ext_needs_a_dot" ~header:(out ^ "default ext: img\n") ""
      [ parse (Invalid_extension "img") ];
    checked "default_ext_is_set_once"
      ~header:(out ^ "default ext: .a\ndefault ext: .b\n")
      ""
      [ parse (Duplicate_setting "default ext") ];
    checked "default_ext_is_set_once_in_a_stage"
      "stage s:\n    default ext: .a\n    default ext: .b\n    copied = copy(raw)\n"
      [ parse (Duplicate_setting "default ext") ];
    test "a_recipe_cannot_set_ext" ~header:out ~sources:[ raw ] ~files:[ "in/a.raw" ]
      (reads [ "raw" ])
      [
        recipe "pipeline pipeline.spit\nroot data\ndefault ext: .img\n"
          [ problem (Recipe_directive "default ext: .img") ];
      ];
    test "an_imported_operation_brings_its_extension" ~header:out ~sources:[ raw ]
      ~files:[ "in/a.raw" ]
      ~texts:[ ("tools.spit", operation "align(input) -> Transform .mat") ]
      "use tools: tools.spit [align]\nmatrix = tools.align(raw)\n"
      [ plan [ at (path 0 "output") (quoted "out/matrix/id=a.mat") ] ];
  ]

let coverage =
  [
    checked "an_output_with_no_rule_is_an_error" ~header:""
      ~sources:[ source "raw" ~typ:"Raw" ~path:"" [ "id" ] ]
      (operation "clean(x: Raw) -> Clean" ^ "cleaned = clean(raw)\n")
      [ problem (Output_no_path_rule "cleaned") ];
    checked "a_source_with_no_rule_waits_for_one" ~header:""
      ~sources:[ source "raw" ~typ:"Raw" ~path:"" [ "id" ] ]
      (operation "clean(x: Raw) -> Clean" ^ "cleaned = clean(raw)\npath cleaned: out/{id}.txt\n")
      [];
    planned "a_source_with_no_rule_is_an_error_when_planned" ~header:""
      ~sources:[ source "raw" ~typ:"Raw" ~path:"" [ "id" ] ]
      (operation "clean(x: Raw) -> Clean" ^ "cleaned = clean(raw)\npath cleaned: out/{id}.txt\n")
      [ problem (Source_no_path_rule "raw") ];
    checked "an_unused_source_with_no_rule_is_fine" ~header:""
      ~sources:[ source "unused" ~path:"" [ "id" ] ]
      "" [];
    checked "a_rule_naming_an_absent_dimension_is_an_error" ~header:""
      ~sources:[ source "unused" ~path:"input/{id}/{missing}.txt" [ "id" ] ]
      ""
      [ problem (Absent_dimension { product = "unused"; dim = "missing"; own_rule = true }) ];
    checked "a_rule_that_omits_a_dimension_is_an_error"
      ~header:("default path: {@product}/{@entities}.csv\n" ^ entities)
      ~sources:[ source "raw" ~path:"raw/{id}.csv" [ "id"; "batch" ] ]
      ""
      [ problem (Omitted_dimension { product = "raw"; dim = "batch" }) ];
    checked "two_products_on_one_rule_cannot_be_told_apart"
      ~header:("default path: {@entities}.csv\n" ^ entities)
      ~sources:[ source "raw" ~path:"" [ "id" ] ]
      "operation clean(input) ->\ncommand clean: clean {input} {@output}\ncleaned = clean(raw)\n"
      [ problem (Same_path { first = "raw"; second = "cleaned"; path = "{@entities}.csv" }) ];
    checked "a_default_naming_a_dimension_an_output_lacks_says_to_give_it_a_rule"
      ~header:"default path: {@product}/{id}.csv\n"
      ~sources:[ source "raw" ~path:"raw/{id}.csv" [ "id" ] ]
      "operation total(raw: many) -> Table\n\
       command total: tool {raw} {@output}\n\
       summary = total(raw @vary(id))\n"
      [ problem (Absent_dimension { product = "summary"; dim = "id"; own_rule = false }) ];
    checked "sources_on_their_own_rules_are_distinct" ~header:""
      ~sources:
        [
          source "raw" ~path:"out/{id}.csv" [ "id" ];
          source "extra" ~path:"out/{batch}.csv" [ "batch" ];
        ]
      "" [];
    test "a_default_naming_a_dimension_a_source_lacks_leaves_it_to_the_recipe"
      ~header:"default path: out/sub-{sub}/{@product}\n" ~sources:[]
      ~files:[ "in/01/a.txt"; "scripts/plot.py" ]
      ("source raw: Text .txt [sub]\nsource plotter: Script .py\n"
     ^ "operation plot(raw: Text, script: Script) -> Figure .png\n\
        command plot: python {script} {raw} {@output}\n\
        fig = plot(raw, plotter)\n")
      [
        recipe
          "pipeline pipeline.spit\n\
           root data\n\
           inputs:\n\
          \    raw: in/{sub}/a.txt\n\
          \    plotter: scripts/plot.py\n"
          [ jobs 1 ];
        recipe "pipeline pipeline.spit\nroot data\ninputs:\n    raw: in/{sub}/a.txt\n"
          [ problem (Source_no_path_rule "plotter") ];
      ];
  ]

let builtins =
  [
    planned "a_dimension_may_take_a_built_in_name"
      ~header:"default path: out/{@stage}/{@product}/{stage}_{product}\n"
      ~sources:[ source "raw" ~typ:"A" ~path:"in/{stage}/{product}" [ "stage"; "product" ] ]
      ~files:[ "in/s/p" ]
      (operation "copy(a: A) -> A" ^ "stage prep:\n    copied = copy(raw)\n")
      [ at (path 0 "output") (quoted "out/prep/copied/s_p") ];
    checked "an_unknown_built_in_is_an_error" ~header:"default path: out/{@name}/{@entities}\n"
      ~sources:[] ""
      [ parse (Unknown_builtin "@name") ];
    planned "optional_groups_resolve_for_each_product"
      ~header:
        ("default path: \
          derivatives/sub-{sub}[/ses-{ses}]/sub-{sub}[_ses-{ses}][_run-{run}]_{@product}.txt\n"
       ^ entities)
      ~sources:
        [ source "bold" ~typ:"Bold" ~path:"raw/{sub}/{ses}/{run}.nii" [ "sub"; "ses"; "run" ] ]
      ~files:[ "raw/01/01/1.nii"; "raw/01/01/2.nii" ]
      (operation "mc(x: Bold) -> Bold"
      ^ operation "average(x: many Bold) -> Bold"
      ^ "mc = mc(bold)\navg = average(mc @vary(run))\nlong = average(avg @vary(ses))\n")
      [
        at "jobs.*.outputs.output.path"
          {|["derivatives/sub-01/ses-01/sub-01_ses-01_run-1_mc.txt","derivatives/sub-01/ses-01/sub-01_ses-01_run-2_mc.txt","derivatives/sub-01/ses-01/sub-01_ses-01_avg.txt","derivatives/sub-01/sub-01_long.txt"]|};
      ];
    checked "an_optional_group_names_a_placeholder" ~header:"default path: out/[x]/{@product}\n"
      ~sources:[] ""
      [ parse Group_without_placeholder ];
    checked "optional_groups_may_nest" ~header:"default path: out/[{ses}[{run}]]\n" ~sources:[] ""
      [];
    checked "an_optional_group_is_closed" ~header:"default path: out/[{ses}\n" ~sources:[] ""
      [ parse Unclosed_bracket ];
    checked "an_optional_group_names_a_dimension_some_product_has"
      ~header:"default path: out/[ses-{sess}]{@product}.txt\n"
      ~sources:[ source "raw" ~path:"" [ "ses" ] ]
      ""
      [ problem (Rule_unknown_dimension { product = "raw"; dim = "sess" }) ];
    planned "a_dimensionless_product_omits_an_optional_entities_group"
      ~header:
        "default path: out/[{@entities}_]{@product}.txt\n\
         default entities: {@key}-{@value} separated \"_\"\n"
      ~sources:[ source "singleton" ~path:"" [] ]
      ~files:[ "out/singleton.txt" ] (reads [ "singleton" ])
      [ at "external_inputs.0.path" (quoted "out/singleton.txt") ];
    checked "a_dimensionless_product_cannot_use_entities_outside_a_group"
      ~header:"default entities: {@key}-{@value} separated \"_\"\n"
      ~sources:[ source "singleton" ~path:"out/{@entities}_{@product}.txt" [] ]
      ""
      [ problem (Entities_without_dimensions "singleton") ];
    planned "discovery_uses_each_sources_resolved_groups_and_entities"
      ~header:
        "default path: in/sub-{sub}[/ses-{ses}]/{@entities}_{@product}.txt\n\
         default entities: {@key}-{@value} separated \"_\"\n"
      ~sources:[ source "image" ~path:"" [ "sub"; "ses" ]; source "metadata" ~path:"" [ "sub" ] ]
      ~files:[ "in/sub-01/ses-02/sub-01_ses-02_image.txt"; "in/sub-01/sub-01_metadata.txt" ]
      (reads [ "image"; "metadata" ])
      [
        has "external_inputs.*.entities" {|{"sub":"01","ses":"02"}|};
        has "external_inputs.*.entities" {|{"sub":"01"}|};
      ];
  ]

let tests = extensions @ coverage @ builtins
