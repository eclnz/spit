open Plan_support
open Problem

let header =
  "default entities: {@key}={@value} separated \"_\"\ndefault path: out/{@product}[/{@entities}]\n"

let plots = [ "input/allotment.txt"; "input/garden.txt" ]

let pipeline =
  "review looked_at\n\
   source list: Lines .txt [plot]\n\
   path list: input/{plot}.txt\n\
   operation tidy(list: Lines) -> Lines @review(looked_at)\n\
   command tidy: sort -u {list} > {@output}\n\
   tidied = tidy(list)\n\
   operation count(list: Lines) -> Count\n\
   command count: wc -l < {list} > {@output}\n\
   counted = count(tidied)\n"

let recipe_text = "pipeline pipeline.spit\nroot data\nreview looked_at from qc/looked_at.csv\n"

let reviewed =
  test "a_review_on_an_output_targets_each_artifact_it_makes" ~header ~files:plots pipeline
    [
      recipe recipe_text
        [
          jobs 4;
          at "reviews.*.name" {|["looked_at"]|};
          at "reviews.0.dimensions" {|["plot"]|};
          at "reviews.0.targets.*.artifact.path"
            {|["out/tidied/plot=allotment","out/tidied/plot=garden"]|};
          at "reviews.0.targets.*.input" "[null,null]";
        ];
    ]

let tests =
  [
    reviewed;
    test "a_review_names_its_file_from_the_recipe" ~header ~files:plots pipeline
      [ recipe recipe_text [ ends "reviews.0.file" "/qc/looked_at.csv" ] ];
    test "a_review_without_a_recipe_has_no_file" ~header ~files:plots pipeline
      [ plan [ at "reviews.0.file" "null"; at "reviews.0.targets.#" "2" ] ];
    test "a_default_review_judges_every_output_unless_one_opts_out" ~header ~files:plots
      "review looked_at\n\
       default review: looked_at\n\
       source list: Lines .txt [plot]\n\
       path list: input/{plot}.txt\n\
       operation tidy(list: Lines) -> Lines\n\
       command tidy: sort -u {list} > {@output}\n\
       tidied = tidy(list)\n\
       operation total(lists: many Lines) -> Lines @review(!looked_at)\n\
       command total: cat {lists} > {@output}\n\
       everything = total(tidied @vary(plot))\n"
      [ plan [ at "reviews.0.targets.*.artifact.product" {|["tidied","tidied"]|} ] ];
    test "a_review_on_an_input_port_targets_each_job_reading_it" ~header ~files:plots
      "review looked_at\n\
       source list: Lines .txt [plot]\n\
       path list: input/{plot}.txt\n\
       operation tidy(list: Lines @review(looked_at)) -> Lines\n\
       command tidy: sort -u {list} > {@output}\n\
       tidied = tidy(list)\n"
      [
        plan
          [
            at "reviews.0.targets.*.artifact.path" {|["input/allotment.txt","input/garden.txt"]|};
            at "reviews.0.targets.*.input" {|[{"job":1,"port":0},{"job":2,"port":0}]|};
          ];
      ];
    test "a_review_judges_only_the_dimensions_its_products_share" ~header
      ~files:[ "input/allotment.txt" ]
      "review looked_at\n\
       source list: Lines .txt [plot]\n\
       path list: input/{plot}.txt\n\
       operation tidy(list: Lines) -> Lines @review(looked_at)\n\
       command tidy: sort -u {list} > {@output}\n\
       tidied = tidy(list)\n\
       operation total(lists: many Lines) -> Lines @review(looked_at)\n\
       command total: cat {lists} > {@output}\n\
       everything = total(tidied @vary(plot))\n"
      [ plan [ at "reviews.0.dimensions" "[]"; at "reviews.0.targets.#" "2" ] ];
    test "an_unknown_review_is_an_error" ~header
      (replace "@review(looked_at)" "@review(looked)" pipeline)
      [ check [ problem (Review_unknown "looked") ] ];
    test "a_recipe_names_only_reviews_the_pipeline_declares" ~header ~files:plots pipeline
      [
        recipe "pipeline pipeline.spit\nroot data\nreview other from qc/other.csv\n"
          [ problem (Recipe_review_unknown "other") ];
      ];
    test "a_recipe_gives_a_review_one_file" ~header ~files:plots pipeline
      [
        recipe
          (recipe_text ^ "review looked_at from qc/again.csv\n")
          [ problem (Recipe_review_twice "looked_at") ];
      ];
  ]
