open Plan_support
open Problem

let header =
  "default entities: {@key}={@value} separated \"_\"\ndefault path: out/{@product}[/{@entities}]\n"

let survey =
  [
    "coast/alder/1.csv";
    "coast/alder/2.csv";
    "coast/alder/calibration.txt";
    "coast/dune/1.csv";
    "coast/dune/2.csv";
    "coast/dune/calibration.txt";
    "coast/dune/calibration_2024.txt";
    "valley/reed/calibration.txt";
    "valley/sedge/1.csv";
    "valley/sedge/3.csv";
    "valley/sedge/calibration.txt";
  ]

let pipeline =
  "source reading: Readings .csv [region, site, day]\n\
   source calibration: Offset .txt [region, site]\n\
   operation clean(days: many Readings, offset: Offset) -> Readings\n\
   command clean: clean {days} {offset} {@output}\n\
   cleaned = clean(reading @vary(day), calibration)\n"

let inputs extra =
  "pipeline pipeline.spit\n\
   root data\n\
   inputs:\n\
  \    reading: {region}/{site}/{day}.csv\n\
  \    calibration: {region}/{site}/calibration*.txt\n\
  \    contexts: {region}/{site}\n" ^ "prefer path */*/calibration.txt\n" ^ extra

let planned name extra expects =
  test name ~header ~files:survey pipeline [ recipe (inputs extra) expects ]

let sites = "jobs.*.outputs.output.entities.site"

let tests =
  [
    planned "prefer_keeps_one_path_of_an_identity_and_records_the_rest" ""
      [
        at sites {|["alder","dune","sedge"]|};
        at "jobs.1.inputs.offset.0.path" {|"coast/dune/calibration.txt"|};
        at "removed.0.product" {|"calibration"|};
        at "removed.0.entities" {|{"region":"coast","site":"dune"}|};
        at "removed.0.rule" {|"prefer path */*/calibration.txt"|};
      ];
    test "two_paths_of_one_identity_without_a_preference_are_an_error" ~header ~files:survey
      pipeline
      [
        recipe
          "pipeline pipeline.spit\n\
           root data\n\
           inputs:\n\
          \    reading: {region}/{site}/{day}.csv\n\
          \    calibration: {region}/{site}/calibration*.txt\n"
          [ problem (Inventory_duplicate "calibration") ];
      ];
    planned "an_exclusion_removes_one_artifact_and_says_why"
      "exclude reading[region=valley,site=sedge,day=3]    # a broken sensor\n"
      [
        at "removed.0.rule" {|"exclude reading[region=valley,site=sedge,day=3]"|};
        at "removed.0.reason" {|"a broken sensor"|};
        at "removed.0.origin" {|"line 8"|};
        at "jobs.2.inputs.days.*.entities.day" {|["1"]|};
      ];
    planned "an_exclusion_that_matches_nothing_is_an_error"
      "exclude reading[region=valley,site=sedge,day=9]\n"
      [ problem (Exclusion_matches_nothing "exclude reading[region=valley,site=sedge,day=9]") ];
    planned "an_exclusion_removes_a_group_from_every_source" "exclude [region=valley]\n"
      [
        at sites {|["alder","dune"]|};
        at "removed.0.product" "null";
        at "removed.0.entities" {|{"region":"valley"}|};
      ];
    planned "a_conditional_exclusion_counts_a_folder_with_no_files"
      "exclude [region, site] where reading count<2\n"
      [
        at sites {|["alder","dune","sedge"]|};
        at "removed.1.entities" {|{"region":"valley","site":"reed"}|};
        at "removed.1.found" "0";
      ];
    planned "has_and_missing_judge_a_group_by_a_value"
      "exclude:\n\
      \    [region, site] where reading has day=3\n\
      \    [region, site] where reading missing day=1\n"
      [ at sites {|["alder","dune"]|}; at "removed.*.entities.site" {|["dune","sedge","reed"]|} ];
    planned "a_group_two_rules_remove_is_recorded_once"
      "exclude:\n\
      \    [region, site] where reading has day=3\n\
      \    [region, site] where reading missing day=2\n"
      [ at sites {|["alder","dune"]|}; at "removed.*.entities.site" {|["dune","sedge","reed"]|} ];
    planned "a_requirement_that_holds_lets_the_plan_through"
      "exclude [region=valley]\nrequire [region] where contexts[region,site] count>=2\n"
      [ at sites {|["alder","dune"]|} ];
    planned "a_requirement_that_fails_names_its_group"
      "require [region, site] where reading count=2\n"
      [
        problem
          (Requirement_failed
             {
               rule = "require [region, site] where reading count=2";
               group = "region=valley,site=reed";
               found = "0";
             });
      ];
    test "a_recipe_default_covers_sources_with_no_rule_of_their_own" ~header
      ~files:[ "in/a.csv"; "lists/a.txt" ]
      "source table: Table .csv [id]\n\
       source list: List .txt [id]\n\
       path table: in/{id}.csv\n\
       operation join(table: Table, list: List) -> Table\n\
       command join: join {table} {list} {@output}\n\
       joined = join(table, list)\n"
      [
        recipe "pipeline pipeline.spit\nroot data\ninputs:\n    default: lists/{id}\n"
          [ at "jobs.0.inputs.list.0.path" {|"lists/a.txt"|} ];
      ];
    test "a_star_matches_within_one_name_and_the_plan_reads_the_file_it_found" ~header
      ~files:[ "in/a_2024.csv" ]
      ("source table: Table .csv [id]\npath table: in/{id}_*.csv\n" ^ reads [ "table" ])
      [
        plan
          [
            at "external_inputs.0.path" {|"in/a_2024.csv"|};
            at "external_inputs.0.entities" {|{"id":"a"}|};
          ];
      ];
  ]

let screen =
  "screen positive: grep -Eq '^[1-9]' {@path}\n\
   source count: Count .txt [site]\n\
   path count: in/{site}.txt\n\
   operation copy(x: Count) -> Count @screen(positive)\n\
   command copy: cp {x} {@output}\n\
   copied = copy(count)\n"

let screens =
  [
    test "an_output_screen_runs_after_its_job_and_judges_every_reader" ~header ~files:[ "in/a.txt" ]
      screen
      [
        plan
          [
            at "jobs.0.screens.0.when" {|"after"|};
            at "jobs.0.screens.0.screen" {|"positive"|};
            at "jobs.0.screens.0.product_scope" "true";
            at "jobs.0.screens.0.command.3" {|[{"path":"out/copied/site=a"}]|};
            at "screen_records" {|{"copied":"screens/copied.csv"}|};
          ];
      ];
    test "an_input_port_screen_judges_one_job_input" ~header ~files:[ "in/a.txt" ]
      (replace "-> Count @screen(positive)" "-> Count"
         (replace "copy(x: Count)" "copy(x: Count @screen(positive))" screen))
      [
        plan
          [
            at "jobs.0.screens.0.when" {|"before"|};
            at "jobs.0.screens.0.product_scope" "false";
            at "screen_records" {|{"count":"screens/count.csv"}|};
          ];
      ];
    test "an_unknown_screen_is_an_error" ~header
      (replace "@screen(positive)" "@screen(negative)" screen)
      [ check [ problem (Screen_unknown "negative") ] ];
  ]

let tests = tests @ screens
