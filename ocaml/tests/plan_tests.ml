let () =
  Plan_support.main ~checks:Output_tests.blob_ids
    [
      ("resolution", Resolution_tests.tests);
      ("matching", Matching_tests.tests);
      ("output", Output_tests.tests);
    ]
