open Plan_support

let combine =
  test "a_many_input_can_share_an_operation_with_single_inputs"
    ~sources:
      [ source "result" ~typ:"Result" [ "site"; "run" ]; source "policy" ~typ:"Policy" [ "site" ] ]
    (operation "combine(results: many Result, policy: Policy) -> Summary"
    ^ "summary = combine(result @vary(run), policy)\n")
    ~data:
      [
        "result[site=A,run=1]";
        "result[site=A,run=2]";
        "result[site=B,run=1]";
        "policy[site=A]";
        "policy[site=B]";
      ]
    [
      plan
        [
          jobs 2;
          at "jobs.0.inputs.results.#" "2";
          at "jobs.0.inputs.policy.0.entities" {|{"site":"A"}|};
          at "jobs.*.outputs.output.entities" {|[{"site":"A"},{"site":"B"}]|};
        ];
    ]

let leaderboard =
  test "one_aggregate_can_vary_two_dimensions_in_product_order"
    ~sources:[ source "summary" ~typ:"Summary" [ "model"; "config" ] ]
    (operation "leaderboard(summaries: many Summary @min(3)) -> Table"
    ^ "overall = leaderboard(summary @vary(model, config))\n")
    ~data:[ "summary[model=b,config=1]"; "summary[model=a,config=10]"; "summary[model=a,config=2]" ]
    [
      plan
        [
          jobs 1;
          at "jobs.0.inputs.summaries.*.entities"
            {|[{"model":"a","config":"2"},{"model":"a","config":"10"},{"model":"b","config":"1"}]|};
          at "jobs.0.outputs.output.entities" "{}";
        ];
    ]

let varies dims varied data =
  test
    ("a_call_varies_" ^ String.concat "_and_" (String.split_on_char ',' (String.concat "," varied)))
    ~sources:[ source "result" ~typ:"Result" dims ]
    (operation "combine(items: many Result) -> Summary"
    ^ "summary = combine(result @vary(" ^ String.concat ", " varied ^ "))\n")
    ~data
    [
      plan
        [
          jobs 1;
          at "jobs.0.inputs.items.#" "2";
          at "jobs.0.outputs.output.entities" {|{"site":"A"}|};
        ];
    ]

let broadcast =
  test "a_missing_broadcast_suggests_the_input_and_dimension"
    ~sources:[ source "runs" ~typ:"Run" [ "sub"; "run" ]; source "dims" ~typ:"Dims" [ "d" ] ]
    ("dimensions: [sub, run, d]\n"
    ^ operation "group_ica(runs: many Run, dims: Dims) -> Summary"
    ^ "summary = group_ica(runs @vary(run), dims)\n")
    [ check [ fails [ "dims @each(d)" ] ] ]

let pair =
  test "an_operation_can_collect_two_products_across_the_same_dimensions"
    ~sources:
      [
        source "height" ~typ:"Height" [ "region"; "tile" ];
        source "slope" ~typ:"Slope" [ "region"; "tile" ];
      ]
    (operation "table(heights: many Height, slopes: many Slope) -> Table"
    ^ "summary = table(height @vary(tile), slope @vary(tile))\n")
    ~data:
      [
        "height[region=a,tile=1]";
        "height[region=a,tile=2]";
        "height[region=b,tile=1]";
        "slope[region=a,tile=1]";
        "slope[region=a,tile=2]";
        "slope[region=b,tile=1]";
      ]
    [
      plan
        [
          jobs 2;
          at "jobs.0.inputs.heights.*.entities.tile" {|["1","2"]|};
          at "jobs.0.inputs.slopes.*.entities.tile" {|["1","2"]|};
          at "jobs.*.outputs.output.entities" {|[{"region":"a"},{"region":"b"}]|};
        ];
    ]

let calibrate call data runs =
  test ""
    ~sources:
      [
        source "signal" ~typ:"Signal" [ "site"; "run" ];
        source "calibration" ~typ:"Calibration" [ "site"; "revision" ];
      ]
    (operation "apply(data: Signal, calibration: Calibration) -> Signal"
    ^ "calibrated = " ^ call ^ "\n")
    ~data runs

let named name (t : test) = { t with name }

let predict =
  test "each_runs_a_step_for_every_value_an_input_broadcasts"
    ~sources:
      [
        source "reading" ~typ:"Series" [ "station" ];
        source "model" ~typ:"Model" [ "scenario" ];
        source "parameters" ~typ:"Parameters" [ "scenario" ];
      ]
    ("dimensions: [station, scenario]\n"
    ^ operation "predict(reading: Series, model: Model, parameters: Parameters) -> Matrix"
    ^ "forecast = predict(reading, model @each(scenario), parameters)\n")
    ~data:
      [
        "reading[station=01]";
        "reading[station=02]";
        "model[scenario=base]";
        "model[scenario=high]";
        "parameters[scenario=base]";
        "parameters[scenario=high]";
      ]
    [
      plan
        [
          at "jobs.*.outputs.output.entities"
            {|[{"station":"01","scenario":"base"},{"station":"01","scenario":"high"},{"station":"02","scenario":"base"},{"station":"02","scenario":"high"}]|};
          at "jobs.1.inputs.parameters.0.entities" {|{"scenario":"high"}|};
        ];
    ]

let fit =
  test "a_driver_that_has_the_broadcast_dimension_is_an_error"
    ~sources:
      [
        source "reading" ~typ:"Series" [ "station"; "scenario" ];
        source "model" ~typ:"Model" [ "scenario" ];
      ]
    (operation "fit(reading: Series, model: Model) -> Series"
    ^ "fitted = fit(reading, model @each(scenario))\n"
    ^ operation "stack(items: many Series, model: Model) -> Stack"
    ^ "stacked = stack(fitted @vary(scenario), model @each(scenario))\n")
    [ check [ fails [ "scenario" ] ] ]

let frames =
  test "collections_and_jobs_follow_natural_declared_order"
    ~sources:[ source "frame" ~typ:"Frame" [ "subject"; "run" ] ]
    (operation "stack(frames: many Frame) -> Stack" ^ "stacked = stack(frame @vary(run))\n")
    ~data:
      [
        "frame[subject=s10,run=10]";
        "frame[subject=s10,run=2]";
        "frame[subject=s2,run=1]";
        "frame[subject=s10,run=1]";
      ]
    [
      plan
        [
          at "jobs.*.outputs.output.entities" {|[{"subject":"s2"},{"subject":"s10"}]|};
          at "jobs.1.inputs.frames.*.entities.run" {|["1","2","10"]|};
        ];
    ]

let baseline =
  test "adding_a_later_artifact_leaves_the_plan_unchanged"
    ~sources:[ source "scan" ~typ:"Image" [ "subject"; "session" ] ]
    (operation "copy(image: Image) -> Image"
    ^ "baseline = copy(scan @first(session))\nresult = copy(baseline)\n")
    ~datasets:
      [
        ("before", [ "scan[subject=a,session=2]" ]);
        ("after", [ "scan[subject=a,session=2]"; "scan[subject=a,session=10]" ]);
      ]
    [
      plan_in "before" [ at "jobs.0.inputs.image.0.path" {|"scan/a/2.txt"|} ];
      plan_in "after" [ same "jobs" ];
    ]

let selection selector =
  List.map
    (fun (clause, words) ->
      vary
        (Printf.sprintf "@%s_rejects_%s" selector clause)
        ~steps:(replace ("@" ^ selector ^ "(session)") clause)
        [ check [ fails words ] ]
        {
          baseline with
          steps = replace "@first(session)" ("@" ^ selector ^ "(session)") baseline.steps;
        })
    [
      ("@" ^ selector ^ "(missing)", [ "missing" ]);
      ("@" ^ selector ^ "(subject, session)", []);
      ("@" ^ selector ^ "(session) @where(session=2)", [ "session" ]);
      ("@" ^ selector ^ "(session) @last(session)", []);
    ]

let picked =
  test "a_body_can_select_from_its_callers_filtered_input"
    ~sources:[ source "scan" ~typ:"Image" [ "subject"; "session" ] ]
    (operation "copy(image: Image) -> Image"
    ^ "operation pick(input: Image) -> (chosen: Image):\n    chosen = copy(input @first(session))\n"
    ^ "baseline = pick(scan @where(subject=a))\nresult = copy(baseline)\n")
    ~data:[ "scan[subject=a,session=10]"; "scan[subject=a,session=2]"; "scan[subject=b,session=1]" ]
    [
      plan
        [
          jobs 2;
          at "jobs.*.outputs.output.entities" "[{},{}]";
          at "jobs.0.inputs.image.0.entities.session" {|"2"|};
        ];
    ]

let minimum =
  test "min_rejects_a_collection_that_is_too_small"
    ~sources:[ source "frame" ~typ:"Frame" [ "subject"; "run" ] ]
    (operation "stack(frames: many Frame @min(2)) -> Stack" ^ "stacked = stack(frame @vary(run))\n")
    ~data:[ "frame[subject=a,run=1]"; "frame[subject=a,run=2]"; "frame[subject=b,run=1]" ]
    [ plan [ fails [ "@min"; "subject=b" ] ] ]

let declarations =
  List.map
    (fun (declaration, words) ->
      test ("min_declaration_" ^ declaration) (declaration ^ "\n") [ check [ fails words ] ])
    [
      ("operation f(a: A) -> B @min(2)", []);
      ("operation f(as: many A @min(0)) -> B", []);
      ("operation f(a: A @min(2)) -> B", [ "many" ]);
      ("operation f(as: many A @min(2) @min(3)) -> B", []);
      ("operation f(as: many A @vary(run)) -> B", []);
      ("operation f(as: many A) -> B @ other", []);
    ]

let tests =
  [
    combine;
    leaderboard;
    vary "two_varied_dimensions_may_be_written_in_either_order"
      ~steps:(replace "@vary(model, config)" "@vary(config, model)")
      [ plan [ jobs 1 ] ]
      leaderboard;
    vary "a_collection_below_its_minimum_is_an_error" ~steps:(replace "@min(3)" "@min(4)")
      [ plan [ fails [ "@min" ] ] ]
      leaderboard;
    vary "vary_is_written_once_per_input"
      ~steps:(replace "@vary(model, config)" "@vary(model) @vary(config)")
      [ check [ fails [] ] ]
      leaderboard;
    vary "vary_names_a_dimension_once"
      ~steps:(replace "@vary(model, config)" "@vary(model, model)")
      [ check [ fails [ "model" ] ] ]
      leaderboard;
    varies [ "site"; "run" ] [ "run" ] [ "result[site=A,run=1]"; "result[site=A,run=2]" ];
    varies [ "site"; "model"; "config" ] [ "model"; "config" ]
      [ "result[site=A,model=b,config=1]"; "result[site=A,model=a,config=2]" ];
    vary "a_single_input_of_an_aggregate_must_match_each_group"
      ~data:[ "result[site=A,run=1]"; "policy[site=B]" ]
      [ plan [ fails [ "missing input"; "policy"; "site=A" ] ] ]
      combine;
    vary "a_single_input_may_not_have_the_varied_dimension"
      ~sources:(redim "policy" [ "site"; "run" ])
      ~data:[]
      [ check [ fails [ "policy"; "run" ] ] ]
      combine;
    broadcast;
    vary "the_suggested_broadcast_is_accepted"
      ~steps:(replace "dims)" "dims @each(d))")
      [ check [] ]
      broadcast;
    pair;
    vary "collections_that_hold_different_artifacts_are_not_paired"
      ~data:
        [
          "height[region=a,tile=1]";
          "height[region=a,tile=2]";
          "height[region=b,tile=1]";
          "slope[region=a,tile=1]";
          "slope[region=b,tile=1]";
        ]
      [ plan [ fails [ "tile=2" ] ] ]
      pair;
    vary "collections_must_vary_over_the_same_dimensions"
      ~steps:(replace "slope @vary(tile)" "slope @vary(region, tile)")
      [ check [ fails [ "same @vary" ] ] ]
      pair;
    vary "paired_collections_must_have_the_same_dimensions"
      ~sources:(redim "slope" [ "region"; "tile"; "band" ])
      ~data:[]
      [ check [ fails [] ] ]
      pair;
    named "where_pins_a_dimension_the_driver_lacks"
      (calibrate "apply(signal, calibration @where(revision=2))"
         [
           "signal[site=A,run=1]";
           "calibration[site=A,revision=1]";
           "calibration[site=A,revision=2]";
         ]
         [
           plan
             [
               jobs 1;
               at "jobs.0.inputs.calibration.0.entities" {|{"site":"A","revision":"2"}|};
               at "jobs.0.outputs.output.entities" {|{"site":"A","run":"1"}|};
             ];
         ]);
    test "where_filters_the_driver_and_removes_its_dimension_from_the_output"
      ~sources:[ source "image" ~typ:"Image" [ "site"; "echo" ] ]
      (operation "keep(image: Image) -> Image" ^ "first = keep(image @where(echo=1))\n")
      ~data:[ "image[site=A,echo=1]"; "image[site=A,echo=2]" ]
      [ plan [ at "jobs.*.outputs.output.entities" {|[{"site":"A"}]|} ] ];
    test "selectors_combine_on_one_binding"
      ~sources:[ source "frame" ~typ:"Frame" [ "subject"; "acq"; "run" ] ]
      (operation "stack(frames: many Frame) -> Stack"
      ^ "stacked = stack(frame @where(acq=fast) @vary(run))\n")
      ~data:
        [
          "frame[subject=a,acq=fast,run=1]";
          "frame[subject=a,acq=fast,run=2]";
          "frame[subject=a,acq=slow,run=1]";
        ]
      [
        plan
          [
            at "jobs.*.outputs.output.entities" {|[{"subject":"a"}]|};
            at "jobs.0.inputs.frames.#" "2";
          ];
      ];
    named "same_matches_on_fewer_dimensions"
      (calibrate "apply(signal, calibration @same(site))"
         [ "signal[site=A,run=1]"; "calibration[site=A,revision=1]" ]
         [ plan [ jobs 1 ] ]);
    named "same_requires_one_artifact"
      (calibrate "apply(signal, calibration @same(site))"
         [
           "signal[site=A,run=1]";
           "calibration[site=A,revision=1]";
           "calibration[site=A,revision=2]";
         ]
         [ plan [ fails [ "calibration" ] ] ]);
    named "where_names_a_dimension_of_the_input"
      (calibrate "apply(signal, calibration @where(visit=1))" [] [ check [ fails [ "visit" ] ] ]);
    named "same_names_an_unpinned_dimension_of_the_input"
      (calibrate "apply(signal, calibration @same(run))" [] [ check [ fails [ "run" ] ] ]);
    named "vary_applies_to_many_inputs"
      (calibrate "apply(signal @vary(run), calibration @where(revision=1))" []
         [ check [ fails [ "@vary" ] ] ]);
    named "a_spaced_annotation_is_an_error"
      (calibrate "apply(signal @ pick(run), calibration)" [] [ check [ fails [] ] ]);
    predict;
    vary "a_missing_broadcast_input_names_its_value"
      ~data:
        [
          "reading[station=01]";
          "reading[station=02]";
          "model[scenario=base]";
          "model[scenario=high]";
          "parameters[scenario=base]";
        ]
      [ plan [ fails [ "parameters"; "scenario=high" ] ] ]
      predict;
    test "a_broadcast_dimension_can_be_collected_again"
      ~sources:[ source "reading" ~typ:"Series" [ "station" ]; source "seed" ~typ:"Seed" [ "rep" ] ]
      ("dimensions: [station, rep]\n"
      ^ operation "simulate(reading: Series, seed: Seed) -> Series"
      ^ "trial = simulate(reading, seed @each(rep))\n"
      ^ operation "average(items: many Series) -> Series"
      ^ "summary = average(trial @vary(rep))\n")
      ~data:[ "reading[station=01]"; "seed[rep=1]"; "seed[rep=2]"; "seed[rep=10]" ]
      [
        plan
          [
            at "jobs.*.outputs.output.entities"
              {|[{"station":"01","rep":"1"},{"station":"01","rep":"2"},{"station":"01","rep":"10"},{"station":"01"}]|};
            at "jobs.3.inputs.items.#" "3";
          ];
      ];
    vary "each_names_an_unpinned_dimension"
      ~steps:(replace "model @each(scenario)" "model @each(station)")
      [ check [ fails [ "station" ] ] ]
      predict;
    vary "each_cannot_follow_a_pin_of_its_dimension"
      ~steps:(replace "model @each(scenario)" "model @where(scenario=base) @each(scenario)")
      [ check [ fails [ "scenario" ] ] ]
      predict;
    vary "two_inputs_cannot_broadcast_one_dimension"
      ~steps:(replace "parameters)" "parameters @each(scenario))")
      [ check [ fails [ "broadcast" ] ] ]
      predict;
    vary "a_step_needs_an_input_that_drives_it"
      ~steps:
        (replace "predict(reading, model @each(scenario), parameters)"
           "predict(reading @each(station), model @each(scenario), parameters @same(scenario))")
      [ check [ fails [] ] ]
      predict;
    vary "each_names_a_dimension_once"
      ~steps:(replace "@each(scenario)" "@each(scenario, scenario)")
      [ check [ fails [ "scenario" ] ] ]
      predict;
    fit;
    vary "a_broadcast_cannot_restore_a_collected_dimension"
      ~steps:(replace "fitted = fit(reading, model @each(scenario))" "fitted = fit(reading, model)")
      [ check [ fails [ "scenario" ] ] ]
      fit;
    test "port_order_does_not_decide_the_driving_input"
      ~sources:
        [
          source "signal" ~typ:"Signal" [ "site"; "run" ];
          source "calibration" ~typ:"Calibration" [ "site" ];
        ]
      (operation "apply(calibration: Calibration, data: Signal) -> Signal"
      ^ "calibrated = apply(calibration, signal)\n")
      ~data:[ "signal[site=A,run=1]"; "signal[site=A,run=2]"; "calibration[site=A]" ]
      [ plan [ jobs 2; at "jobs.0.outputs.output.entities" {|{"site":"A","run":"1"}|} ] ];
    frames;
    test "first_and_last_choose_from_each_ragged_group_after_filtering"
      ~sources:
        [
          source "scan" ~typ:"Image" [ "session"; "subject"; "acquisition" ];
          source "signal" ~typ:"Signal" [ "session"; "subject" ];
        ]
      (operation "copy(image: Image) -> Image"
      ^ operation "compare(signal: Signal, reference: Image) -> Result"
      ^ "baseline = copy(scan @first(session) @where(acquisition=fast))\n\
         comparison = compare(signal, scan @where(acquisition=fast) @last(session))\n")
      ~data:
        [
          "scan[session=10,subject=a,acquisition=fast]";
          "scan[session=2,subject=a,acquisition=fast]";
          "scan[session=1,subject=a,acquisition=slow]";
          "scan[session=20,subject=a,acquisition=slow]";
          "scan[session=1,subject=b,acquisition=fast]";
          "signal[session=2,subject=a]";
          "signal[session=10,subject=a]";
          "signal[session=1,subject=b]";
        ]
      [
        plan
          [
            at "jobs.*.outputs.output.product"
              {|["baseline","baseline","comparison","comparison","comparison"]|};
            at "jobs.*.outputs.output.entities.subject" {|["a","b","b","a","a"]|};
            at "jobs.2.outputs.output.entities.session" {|"1"|};
            at "jobs.4.outputs.output.entities.session" {|"10"|};
            at "jobs.0.inputs.image.0.entities.session" {|"2"|};
            at "jobs.1.inputs.image.0.entities.session" {|"1"|};
            at "jobs.2.inputs.reference.0.entities.session" {|"1"|};
            at "jobs.3.inputs.reference.0.entities.session" {|"10"|};
            at "jobs.4.inputs.reference.0.entities.session" {|"10"|};
          ];
      ];
    baseline;
    picked;
    vary "a_body_input_takes_one_selector"
      ~steps:(replace "@where(subject=a)" "@last(session)")
      [ check [ fails [] ] ]
      picked;
    minimum;
  ]
  @ selection "first" @ selection "last" @ declarations
