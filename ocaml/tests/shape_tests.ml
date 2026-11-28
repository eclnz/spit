open Plan_support
open Problem

let logs =
  [
    "logs/web1/2026-09-01.log";
    "logs/web1/2026-09-02.log";
    "logs/web2/2026-09-01.log";
    "logs/web1/notes.log";
    "logs/web2/readme.log";
    "logs/web1/2026-9-1.log";
    "logs/web1/20260901.log";
    "logs/web1/2026-09-01-final.log";
    "logs/web1/2026_09_02.log";
    "logs/web1/2026-02-30.log";
    "logs/web1/2026-09-01.log.gz";
    "logs/README.txt";
  ]

let log rule = source "log" ~path:rule [ "server"; "date" ]

let found ?gap name sources files expects =
  test name ?gap ~sources ~files
    (reads (List.map (fun (s : source) -> s.name) sources))
    [ plan expects ]

let inputs = "external_inputs.*.entities"

let checked ?gap name sources steps expects = test name ?gap ~sources steps [ check expects ]

let output = "operation f(s) ->\ncommand f: f {s} {@output}\no = f(s)\n"

let tests =
  [
    found "a_rule_without_a_shape_reads_every_name"
      [ log "logs/{server}/{date}.log" ]
      logs
      [
        at "external_inputs.#" "10";
        has inputs {|{"server":"web1","date":"notes"}|};
        has inputs {|{"server":"web1","date":"2026-09-01-final"}|};
      ];
    found "a_date_shape_reads_only_real_dates"
      [ log "logs/{server}/{date:date}.log" ]
      logs
      [
        at "external_inputs.*.path"
          {|["logs/web1/2026-09-01.log","logs/web1/2026-09-02.log","logs/web2/2026-09-01.log"]|};
      ];
    found "a_file_failing_the_shape_is_counted_as_unmatched"
      ~gap:"the unmatched-file report waits for #418 to settle `spit inputs`"
      [ log "logs/{server}/{date:date}.log" ]
      logs
      [ at "unmatched.#" "9" ];
    test "the_nearest_file_shows_the_rule_with_its_shape"
      ~gap:"nearest-file hints are ported with missed_sources"
      ~sources:[ log "logs/{server}/{date:date}.log" ]
      ~files:[ "logs/web1/notes.log" ] (reads [ "log" ])
      [
        plan
          [
            fails
              [
                "after `logs/web1/`, the file has `notes.log` where the rule has `{date:date}.log`";
              ];
          ];
      ];
    found "adjacent_placeholders_split_where_a_fixed_shape_ends"
      [ source "s" ~path:"in/{date:date}-{run:digits}.txt" [ "date"; "run" ] ]
      [ "in/2026-09-01-3.txt"; "in/2026-09-01-x.txt" ]
      [ at inputs {|[{"date":"2026-09-01","run":"3"}]|} ];
    found "a_year_then_digits_need_no_separator"
      [ source "s" ~path:"in/{year:year}{n:digits}.txt" [ "year"; "n" ] ]
      [ "in/202401.txt"; "in/2124.txt" ]
      [ at inputs {|[{"year":"2024","n":"01"}]|} ];
    found "a_shape_applies_to_part_of_a_name"
      [ source "s" ~path:"sub-{sub:digits}_T1w.nii" [ "sub" ] ]
      [ "sub-01_T1w.nii"; "sub-ab_T1w.nii"; "sub-01-02_T1w.nii" ]
      [ at inputs {|[{"sub":"01"}]|} ];
    found "a_dimension_written_twice_keeps_its_shape_in_both_places"
      [ source "s" ~path:"{id:digits}/{id}.txt" [ "id" ] ]
      [ "7/7.txt"; "a/a.txt"; "7/8.txt" ]
      [ at inputs {|[{"id":"7"}]|} ];
    found "a_leading_zero_is_kept"
      [ source "s" ~path:"r/run-{run:digits}.txt" [ "run" ] ]
      [ "r/run-007.txt" ]
      [ at inputs {|[{"run":"007"}]|} ];
    found "two_rules_that_both_read_a_file_still_conflict"
      [
        source "dated" ~path:"logs/{date:date}.log" [ "date" ];
        source "named" ~path:"logs/{name}.log" [ "name" ];
      ]
      [ "logs/2026-09-01.log" ]
      [
        problem (Claimed_twice { path = "logs/2026-09-01.log"; first = "dated"; second = "named" });
      ];
    test "a_recipe_rule_takes_a_shape"
      ~sources:[ source "s" ~path:"" [ "day" ] ]
      ~files:[ "2026-09-01/a.txt"; "old/b.txt"; "2026-09-02/a.txt" ]
      (reads [ "s" ])
      [
        recipe "pipeline pipeline.spit\nroot data\ninputs:\n    s: {day:date}/a.txt\n"
          [ at inputs {|[{"day":"2026-09-01"},{"day":"2026-09-02"}]|} ];
      ];
    test "a_discover_rule_takes_a_shape" ~gap:"`contexts:` in a recipe is ported with discovery"
      ~sources:[ source "s" ~path:"{day:date}/a.txt" [ "day" ] ]
      ~files:[ "2026-09-01/a.txt"; "old/b.txt" ]
      (reads [ "s" ])
      [ recipe "pipeline pipeline.spit\nroot data\ninputs:\n    contexts: {day:date}\n" [ jobs 1 ] ];
    checked "a_bad_shape_says_which_shapes_there_are"
      [ source "s" ~path:"in/{d:dat}.txt" [ "d" ] ]
      ""
      [ parse (Unknown_shape { shape = "dat"; placeholder = "d:dat" }) ];
    checked "a_built_in_placeholder_takes_no_shape"
      [ source "s" ~path:"in/{@entities:digits}.txt" [ "d" ] ]
      ""
      [ parse Builtin_shape ];
    checked "one_dimension_has_one_shape"
      [ source "s" ~path:"in/{d:date}/{d:year}.txt" [ "d" ] ]
      ""
      [ problem (Two_shapes { product = "s"; dim = "d"; first = Date; second = Year }) ];
    checked "two_open_shapes_need_text_between"
      [ source "s" ~path:"in/{a:digits}{b:digits}.txt" [ "a"; "b" ] ]
      ""
      [ problem (Touching_shapes { product = "s"; dropped = false }) ];
    checked "a_shape_on_an_output_is_an_error"
      [ source "s" ~path:"in/{d}.txt" [ "d" ] ]
      (output ^ "path o: out/{d:digits}.txt\n")
      [ problem (Shape_on_output "o") ];
    test "a_shape_in_a_default_path_names_the_rule_to_write"
      ~header:"default path: out/{@product}/{d:digits}\n"
      ~sources:[ source "s" ~path:"in/{d}.txt" [ "d" ] ]
      output
      [ check [ problem Shape_in_default ] ];
    checked "a_shape_in_a_source_rule_beside_a_default_path_is_fine"
      [ source "s" ~path:"in/{d:digits}.txt" [ "d" ] ]
      "" [];
    checked "two_sources_with_one_dimension_name_collide_whatever_their_shapes"
      [ source "a" ~path:"in/{d:date}.log" [ "d" ]; source "b" ~path:"in/{d:digits}.log" [ "d" ] ]
      ""
      [ problem (Same_path { first = "a"; second = "b"; path = "in/{d}.log" }) ];
    checked "two_sources_with_different_dimension_names_do_not_collide"
      [ source "a" ~path:"in/{d:date}.log" [ "d" ]; source "b" ~path:"in/{n:digits}.log" [ "n" ] ]
      "" [];
    test "a_dropped_group_can_leave_two_open_shapes_touching"
      ~gap:"`default:` in a recipe is ported with the inputs tests"
      ~sources:[ source "s" ~path:"" [ "a"; "b" ] ]
      ""
      [
        recipe
          "pipeline pipeline.spit\n\
           root data\n\
           inputs:\n\
          \    default: in/{a:digits}[-{c}_]{b:digits}.txt\n"
          [ problem (Touching_shapes { product = "s"; dropped = true }) ];
      ];
    checked "a_dropped_group_in_a_source_rule_can_leave_two_open_shapes_touching"
      [ source "s" ~path:"in/{a:digits}[-{c}_]{b:digits}.txt" [ "a"; "b" ] ]
      ""
      [ problem (Touching_shapes { product = "s"; dropped = true }) ];
    checked "a_dropped_group_leaving_a_separator_is_fine"
      [ source "s" ~path:"in/{a:digits}-[{c}-]{b:digits}.txt" [ "a"; "b" ] ]
      "" [];
  ]
  @ List.map
      (fun rule ->
        checked
          ("a_value_of_any_length_beside_another_is_an_error_in_" ^ rule)
          [ source "s" ~path:rule [ "a"; "b" ] ]
          ""
          [
            problem (Touching_values { product = "s"; dropped = false; first = "a"; second = "b" });
          ])
      [ "in/{a:digits}{b}.txt"; "in/{a}{b}.txt"; "in/{a}{b:digits}.txt" ]
  @ [
      found "a_fixed_length_shape_beside_an_unshaped_placeholder_splits_where_it_ends"
        [
          source "s" ~path:"in/{y:year}{b}.txt" [ "y"; "b" ];
          source "t" ~path:"in/{b}{y:year}.txt" [ "y"; "b" ];
        ]
        [ "in/2024x.txt"; "in/x2024.txt" ]
        [ has inputs {|{"y":"2024","b":"x"}|}; at "external_inputs.*.product" {|["s","t"]|} ];
      checked "an_output_may_put_placeholders_side_by_side"
        [ source "s" ~path:"in/{a}-{b}.txt" [ "a"; "b" ] ]
        "operation f(s) ->\ncommand f: f {s} {@output}\no = f(s)\npath o: out/{a}{b}.txt\n" [];
      found "a_date_on_a_leap_day_follows_the_century_rule"
        [ source "s" ~path:"in/{d:date}.log" [ "d" ] ]
        [
          "in/1900-02-29.log";
          "in/2000-02-29.log";
          "in/2024-02-29.log";
          "in/2100-02-29.log";
          "in/1899-01-01.log";
          "in/2100-01-01.log";
          "in/2023-02-29.log";
        ]
        [ at "external_inputs.*.entities.d" {|["2000-02-29","2024-02-29"]|} ];
      test "a_folder_source_takes_a_shape" ~gap:"folder sources are ported with folders"
        ~files:[ "runs/2024/a.txt"; "runs/2025/a.txt"; "runs/abc/a.txt"; "runs/2124/a.txt" ]
        "source s: T / [y]\npath s: runs/{y:year}\n"
        [ plan [ at "external_inputs.#" "2" ] ];
      test "a_beside_companion_inherits_the_shape" ~gap:"beside sources are ported with sidecars"
        ~files:
          [
            "site-a/2024-01-05.raw";
            "site-a/2024-01-05.json";
            "site-a/notes.raw";
            "site-a/notes.json";
          ]
        ("source raw: .raw [site, d]\n\
          path raw: site-{site}/{d:date}.raw\n\
          source meta: .json beside raw\n"
        ^ reads [ "raw"; "meta" ])
        [ plan [ at "external_inputs.#" "2" ] ];
      found "a_shape_inside_an_optional_group_applies_where_the_group_is_kept"
        [
          source "s" ~path:"in/{d:date}.txt" [ "d" ];
          source "t" ~path:"in/{d:date}[-{v:digits}].txt" [ "d"; "v" ];
        ]
        [
          "in/2024-01-05.txt"; "in/2024-01-05-2.txt"; "in/2024-01-06-xx.txt"; "in/2024-01-07-3.txt";
        ]
        [
          has inputs {|{"d":"2024-01-05"}|};
          has inputs {|{"d":"2024-01-05","v":"2"}|};
          has inputs {|{"d":"2024-01-07","v":"3"}|};
          at "external_inputs.#" "3";
        ];
      test "a_recipes_shared_default_takes_a_shape"
        ~gap:"`default:` in a recipe is ported with the inputs tests"
        ~sources:[ source "s" ~path:"" [ "d" ] ]
        ~files:[ "in/2024-01-05.txt"; "in/2024-01-06.txt"; "in/notes.txt" ]
        (reads [ "s" ])
        [
          recipe "pipeline pipeline.spit\nroot data\ninputs:\n    default: in/{d:date}.txt\n"
            [ jobs 2 ];
        ];
    ]
