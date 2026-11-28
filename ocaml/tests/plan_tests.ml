let () =
  Plan_support.main ~checks:Output_tests.blob_ids
    [
      ("resolution", Resolution_tests.tests);
      ("matching", Matching_tests.tests);
      ("output", Output_tests.tests);
      ("shapes", Shape_tests.tests);
      ("paths", Path_tests.tests);
      ("entities", Entity_tests.tests);
      ("companions", Companion_tests.tests);
      ("folders", Folder_tests.tests);
      ("reviews", Review_tests.tests);
      ("input rules", Input_rule_tests.tests);
    ]
