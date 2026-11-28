open Plan_support
open Problem

let signal = source "signal" ~typ:"Signal" [ "site"; "day"; "run" ]

let calibration = source "calibration" ~typ:"Calibration" [ "site"; "day" ]

let denoise =
  "operation denoise(input: Signal) -> FilteredSignal\ncommand denoise: denoise {input} {@output}\n"

let register =
  "operation register(moving: FilteredSignal, reference: Calibration) -> AlignedSignal\n\
   command register: align {moving} {reference} {@output}\n"

let mean =
  "operation mean(input: many AlignedSignal) -> MeanSignal\n\
   command mean: average {input} {@output}\n"

let registration =
  test "reuses_less_specific_input_across_runs"
    ~sources:[ source "denoised" ~typ:"FilteredSignal" [ "site"; "day"; "run" ]; calibration ]
    (register ^ "registered = register(denoised, calibration)\n")
    ~data:
      [
        "denoised[site=01,day=01,run=1]";
        "denoised[site=01,day=01,run=2]";
        "calibration[site=01,day=01]";
      ]
    [
      plan
        [
          jobs 2;
          at "jobs.*.inputs.reference.0.path" {|["calibration/01/01.txt","calibration/01/01.txt"]|};
          at "jobs.*.outputs.output.entities.run" {|["1","2"]|};
        ];
    ]

let full =
  test "tracks_dependencies_through_a_pipeline" ~sources:[ signal; calibration ]
    (denoise ^ register ^ mean
   ^ "denoised = denoise(signal)\n\
      registered = register(denoised, calibration)\n\
      mean_signal = mean(registered @vary(run))\n")
    ~data:
      [
        "signal[site=01,day=01,run=1]";
        "signal[site=01,day=01,run=2]";
        "calibration[site=01,day=01]";
      ]
    [
      plan
        [
          jobs 5;
          at "jobs.*.operation" {|["denoise","denoise","register","register","mean"]|};
          at "jobs.*.depends_on" "[[],[],[1],[2],[3,4]]";
          at "jobs.4.inputs.input.*.entities.run" {|["1","2"]|};
          at "jobs.4.outputs.output.entities" {|{"site":"01","day":"01"}|};
        ];
    ]

let combine =
  test "rejects_an_accidental_cartesian_product"
    ~sources:[ source "a" ~typ:"A" [ "site"; "run" ]; source "b" ~typ:"B" [ "site"; "echo" ] ]
    "operation combine(a: A, b: B) -> C\n\
     command combine: join {a} {b} {@output}\n\
     c = combine(a, b)\n"
    [ check [ problem (Driver_lacks { input = "b"; driver = Some "a"; dims = [ "echo" ] }) ] ]

let table =
  "source raw: Table .txt [id]\npath raw: raw/{id}.txt\n"
  ^ "operation clean(table: Table) -> Table\ncommand clean: clean {table} {@output}\n"

let tests =
  [
    test "expands_one_to_one_over_two_runs" ~sources:[ signal ]
      (denoise ^ "denoised = denoise(signal)\n")
      ~data:[ "signal[site=01,day=01,run=1]"; "signal[site=01,day=01,run=2]" ]
      [
        plan
          [
            jobs 2;
            at "jobs.*.outputs.output.product" {|["denoised","denoised"]|};
            at "jobs.*.outputs.output.entities.run" {|["1","2"]|};
          ];
      ];
    registration;
    vary "reports_missing_input"
      ~data:[ "denoised[site=01,day=02,run=1]"; "calibration[site=01,day=01]" ]
      [
        plan
          [
            problem
              (Missing_input { port = "reference"; operation = "register"; at = "site=01,day=02" });
          ];
      ]
      registration;
    vary "rejects_an_input_with_dimensions_the_driver_lacks"
      ~sources:(redim "calibration" [ "site"; "day"; "mode" ])
      ~data:[]
      [
        check
          [
            problem
              (Driver_lacks { input = "calibration"; driver = Some "denoised"; dims = [ "mode" ] });
          ];
      ]
      registration;
    vary "checks_types_before_expansion"
      ~steps:(replace "register(denoised, calibration)" "register(denoised, denoised)")
      [ check [ problem (Type_mismatch "reference") ] ]
      registration;
    test "aggregates_each_fixed_dimension_group"
      ~sources:[ source "registered" ~typ:"AlignedSignal" [ "site"; "day"; "run" ] ]
      (mean ^ "mean_signal = mean(registered @vary(run))\n")
      ~data:
        [
          "registered[site=01,day=01,run=1]";
          "registered[site=01,day=01,run=2]";
          "registered[site=02,day=01,run=1]";
          "registered[site=02,day=01,run=2]";
        ]
      [
        plan
          [
            jobs 2;
            at "jobs.*.inputs.input.#" "[2,2]";
            at "jobs.*.outputs.output.entities"
              {|[{"site":"01","day":"01"},{"site":"02","day":"01"}]|};
          ];
      ];
    combine;
    full;
    test "catches_a_product_cycle"
      ~sources:[ source "raw" ~typ:"A" [ "site" ] ]
      "operation copy(input: A) -> A\n\
       command copy: cp {input} {@output}\n\
       a = copy(b)\n\
       b = copy(a)\n"
      [ check [ problem Cycle ] ];
    {
      full with
      name = "the_dataset_changes_the_job_count_and_not_the_pipeline";
      data =
        [
          ("one", [ "signal[site=01,day=01,run=1]"; "calibration[site=01,day=01]" ]);
          ( "two",
            [
              "signal[site=01,day=01,run=1]";
              "signal[site=01,day=01,run=2]";
              "calibration[site=01,day=01]";
            ] );
        ];
      runs = [ plan_in "one" [ jobs 3 ]; plan_in "two" [ jobs 5 ] ];
    };
    test "named_ports_and_the_aggregate_shape_are_checked"
      ~sources:[ source "raw" [ "site"; "run" ] ]
      "operation combine(runs: many) ->\n\
       command combine: cat {runs} > {@output}\n\
       result = combine(raw @vary(run))\n"
      ~data:[ "raw[site=01,run=2]"; "raw[site=01,run=1]" ]
      [
        plan
          [
            jobs 1;
            at "jobs.0.inputs.runs.*.entities.run" {|["1","2"]|};
            at "jobs.0.outputs.output.entities" {|{"site":"01"}|};
          ];
      ];
    test "a_many_input_names_its_dimensions_at_the_call"
      ~sources:[ source "raw" ~typ:"Mean" [ "site"; "run" ] ]
      "operation combine(runs: many Mean) -> Mean\n\
       command combine: cat {runs} > {@output}\n\
       result = combine(raw)\n"
      [ check [ problem (Many_selectors "runs") ] ];
    test "a_misspelled_input_fails_without_data"
      (table ^ "cleaned = clean(rwa)\n")
      [ check [ problem (Unknown_input "rwa") ] ];
    test "a_type_mismatch_fails_without_data"
      (table ^ "source other: Other .txt [id]\npath other: other/{id}.txt\ncleaned = clean(other)\n")
      [ check [ problem (Type_mismatch "table") ] ];
  ]
