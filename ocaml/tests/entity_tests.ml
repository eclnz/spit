open Plan_support

let format = "default entities: {@key}_{@value} separated \"-\"\nentities sub: subject\n"

let flow =
  "operation copy(x) ->\n\
   command copy: cp {x} {@output}\n\
   operation merge(x: many) ->\n\
   command merge: cat {x} > {@output}\n\
   copied = copy(raw)\n\
   session = merge(copied @vary(run))\n\
   subject = merge(session @vary(ses))\n\
   all = merge(subject @vary(sub))\n"

let raw path = source "raw" ~path [ "sub"; "ses"; "run" ]

let outputs = "jobs.*.outputs.output.path"

let declaration ?gap name text words =
  test name ?gap ~header:""
    ~sources:[ source "raw" ~path:"in/{sub}/{ses}" [ "sub"; "ses" ] ]
    text
    [ check [ fails words ] ]

let tests =
  [
    test "one_format_covers_each_shape_without_changing_identity"
      ~header:(format ^ "default path: {@product}[/{@entities}].txt\n")
      ~sources:[ raw "" ]
      ~files:[ "raw/subject_A%5FB-ses_01-run_2.txt" ]
      flow
      [
        plan
          [
            at "external_inputs.0.entities" {|{"sub":"A_B","ses":"01","run":"2"}|};
            at outputs
              {|["copied/subject_A%5FB-ses_01-run_2.txt","session/subject_A%5FB-ses_01.txt","subject/subject_A%5FB.txt","all.txt"]|};
          ];
      ];
    test "a_dimensionless_product_drops_an_optional_group"
      ~header:
        "default entities: {@key}_{@value} separated \"-\"\n\
         default path: out/[{@entities}_]{@product}.txt\n"
      ~sources:[ raw "raw/{@entities}.txt" ]
      ~files:[ "raw/sub_1-ses_2-run_3.txt" ] flow
      [
        plan
          [
            at outputs
              {|["out/sub_1-ses_2-run_3_copied.txt","out/sub_1-ses_2_session.txt","out/sub_1_subject.txt","out/all.txt"]|};
          ];
      ];
    test "an_output_rule_uses_the_format" ~header:format
      ~sources:[ raw "raw/{sub}/{ses}/{run}.txt" ]
      ~files:[ "raw/1/2/3.txt" ]
      (flow ^ "default path: out/{@product}[/{@entities}]\n")
      [ plan [ at "jobs.2.outputs.output.path" {|"out/subject/subject_1"|} ] ];
    test "a_dimensionless_source_cannot_use_entities_outside_a_group"
      ~header:"default entities: {@key}={@value} separated \"__\"\n"
      ~sources:[ source "config" ~path:"in/{@entities}.txt" [] ]
      ""
      [
        check
          [
            fails
              [
                "path rule for `config` uses `{@entities}`, but `config` has no dimensions; put it \
                 in `[...]`, as `[{@entities}_]`";
              ];
          ];
      ];
    test "discovery_decodes_values_and_escaped_aliases_and_accepts_assignment_affixes"
      ~header:"default entities: e_{@key}:{@value}! separated \"-\"\nentities sub: subject_id\n"
      ~sources:[ source "raw" ~path:"raw/{@entities}.txt" [ "sub"; "ses" ] ]
      ~files:[ "raw/e_subject%5Fid:A%5Fb%2F%C3%A9!-e_ses:01!.txt" ]
      "operation copy(x) ->\n\
       command copy: cp {x} {@output}\n\
       copied = copy(raw)\n\
       path copied: out/{@product}/{@entities}\n"
      [
        plan
          [
            at "external_inputs.0.entities" {|{"sub":"A_b/é","ses":"01"}|};
            at "jobs.0.outputs.output.path"
              {|"out/copied/e_subject%5Fid:A%5Fb%2F%C3%A9!-e_ses:01!"|};
          ];
      ];
    test "a_stage_spelling_covers_its_products_and_the_stages_nested_in_it"
      ~header:
        "default entities: {@key}_{@value} separated \"-\"\n\
         default path: out/{@product}/{@entities}.txt\n"
      ~sources:[ source "raw" ~path:"in/{@entities}.txt" [ "sub"; "ses" ] ]
      ~files:[ "in/sub_1-ses_2.txt" ]
      "operation copy(x) ->\n\
       command copy: cp {x} {@output}\n\
       first = copy(raw)\n\
       stage post:\n\
      \    default entities: {@key}={@value} separated \"__\"\n\
      \    entities sub: subject\n\
      \    second = copy(first)\n\
      \    stage inner:\n\
      \        third = copy(second)\n"
      [
        plan
          [
            at outputs
              {|["out/first/sub_1-ses_2.txt","out/second/subject=1__ses=2.txt","out/third/subject=1__ses=2.txt"]|};
          ];
      ];
    test "a_recipe_spelling_spells_the_recipes_own_source_rules"
      ~gap:"`default entities` in a recipe is ported with the inputs tests"
      ~header:
        "default entities: {@key}_{@value} separated \"-\"\n\
         default path: out/{@product}/{@entities}.txt\n"
      ~sources:[ source "raw" ~path:"" [ "sub"; "ses" ] ]
      ~files:[ "rec/sub-1_ses-2.txt" ]
      "operation copy(x) ->\ncommand copy: cp {x} {@output}\nfirst = copy(raw)\n"
      [
        recipe
          "pipeline pipeline.spit\n\
           root data\n\
           default entities: {@key}-{@value} separated \"_\"\n\
           inputs:\n\
          \    raw: rec/{@entities}.txt\n"
          [
            at "external_inputs.0.path" {|"rec/sub-1_ses-2.txt"|};
            at outputs {|["out/first/sub_1-ses_2.txt"]|};
          ];
      ];
    test "a_default_that_needs_entities_leaves_a_dimensionless_source_to_its_own_rule"
      ~header:
        "default entities: {@key}_{@value} separated \"-\"\n\
         default path: out/{@product}/{@entities}.txt\n"
      ~sources:[ source "raw" ~path:"" [ "sub" ]; source "config" ~path:"" [] ]
      ""
      [ check [] ];
    declaration "an_entity_label_needs_a_format" "entities sub: subject\n"
      [ "an entity label renames a dimension in a `default entities:` spelling, and there is none" ];
    declaration "no_product_is_named_entities"
      "default entities: {@key}_{@value} separated \"-\"\n\
       operation copy(x) ->\n\
       command copy: cp {x} {@output}\n\
       entities = copy(raw)\n"
      [ "entities" ];
  ]
  @ List.map
      (fun (text, words) -> declaration ("a_bad_entities_declaration_" ^ text) (text ^ "\n") words)
      [
        ("default entities: {@key} separated \"_\"", [ "{@value}" ]);
        ("default entities: {@value}_{@key} separated \"-\"", [ "{@value}" ]);
        ("default entities: {@key}_{@value}_{@value} separated \"-\"", [ "once" ]);
        ("default entities: {@key}_{unknown} separated \"-\"", []);
        ("default entities: {key}_{value} separated \"-\"", [ "{@key}" ]);
        ("default entities: {@key}_{@value} separated by \"-\"", []);
        ("default entities: {@key}_{@value}", [ "separated" ]);
        ("default entities: {@key}_{@value} separated \"/\"", [ "one path component" ]);
        ("default entities: {@key}_{@value} separated \"%\"", [ "one path component" ]);
        ("default entities: {@key}{@value} separated \"-\"", [ "ambiguous" ]);
        ("entities: {@key}_{@value} separated \"-\"", [ "needs a name" ]);
        ("default entities: {@key}_{@value} separated \"-\"\nentities other: subject", [ "other" ]);
        ("default entities: {@key}_{@value} separated \"-\"\nentities sub: ses", [ "ses" ]);
        ( "default entities: {@key}_{@value} separated \"-\"\n\
           entities sub: subject\n\
           entities sub: participant",
          [ "sub" ] );
        ( "default entities: {@key}_{@value} separated \"-\"\n\
           default entities: {@key}={@value} separated \"__\"",
          [ "default entities"; "more than once" ] );
      ]
