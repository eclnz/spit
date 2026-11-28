open Plan_support
open Problem

let header =
  "default entities: {@key}={@value} separated \"_\"\ndefault path: output/{@product}/{@entities}\n"

let trips = [ "input/alpha/day1/morning.txt"; "input/alpha/evening.txt"; "input/beta/noon.txt" ]

let trip = "source trip: Notes / [trip]\npath trip: input/{trip}\n"

let copy =
  "operation copy(notes: Notes) -> Album /\n\
   command copy: cp -R {notes}/. {@output}\n\
   album = copy(trip)\n"

let albums =
  test "a_folder_source_is_found_and_given_to_a_command_by_its_path" ~header ~files:trips
    (trip ^ copy)
    [
      plan
        [
          jobs 2;
          at "jobs.*.inputs.notes.0.path" {|["input/alpha","input/beta"]|};
          at "jobs.*.inputs.notes.0.kind" {|["folder","folder"]|};
          at "jobs.0.outputs.output.path" {|"output/album/trip=alpha"|};
          at "jobs.0.outputs.output.kind" {|"folder"|};
          at "jobs.0.commands.0.command.2" {|[{"path":"input/alpha"},"/."]|};
        ];
    ]

let bundle =
  "source mean: Mean .txt [site]\n\
   path mean: in/{site}.txt\n\
   operation bundle(means: many Mean) -> Bundle /\n\
   command bundle: cp {means} {@output}/\n\
   bundled = bundle(mean @vary(site))\n\
   path bundled: out/bundled\n"

let catalogue =
  {|operation catalogue(bundle: Bundle) -> Catalogue / from bundle
command catalogue: sh -c 'ls "$1" > "$1/index.txt"' sh {@output}
catalogued = catalogue(bundled)
path catalogued: out/catalogued
|}

let pack =
  "source mean: Mean .txt [site]\n\
   path mean: in/{site}.txt\n\
   source card: Text .card [site]\n\
   path card: in/{site}.card\n\
   operation copy(x) ->\n\
   command copy: cp {x} {@output}\n\
   described = copy(mean)\n\
   labelled = copy(card)\n\
   path described: pack/{site}/mean.txt\n\
   path labelled: pack/{site}/card.txt\n\
   operation pack(mean: Text, card: Text) -> Pack / view\n\
   packed = pack(described, labelled)\n\
   path packed: pack/{site}\n\
   operation read_pack(pack: Pack) -> Text\n\
   command read_pack: cat {pack}/mean.txt {pack}/card.txt > {@output}\n\
   read = read_pack(packed)\n\
   path read: read/{site}.txt\n"

let viewed =
  test "a_directory_view_has_members_and_no_job_of_its_own" ~header:""
    ~files:[ "in/dune.txt"; "in/dune.card" ] pack
    [
      plan
        [
          jobs 3;
          at "jobs.*.operation" {|["copy","copy","read_pack"]|};
          at "jobs.2.inputs.pack.0.kind" {|"directory_view"|};
          at "jobs.2.inputs.pack.0.path" {|"pack/dune"|};
          at "jobs.2.inputs.pack.0.under" {|"output_root"|};
          at "jobs.2.inputs.pack.0.members.*.path" {|["mean.txt","card.txt"]|};
          at "jobs.2.depends_on" "[1,2]";
          at "jobs.0.outputs.output.id" "2";
          at "jobs.2.inputs.pack.0.members.*.id" "[2,3]";
          at "external_inputs.*.kind" {|["file","file"]|};
        ];
    ]

let tests =
  [
    albums;
    vary "a_missing_source_folder_is_an_error" ~data:[]
      [ plan [ problem (No_files "trip") ] ]
      { albums with files = [] };
    test "a_folder_takes_no_default_extension" ~header:(header ^ "default ext: .txt\n") ~files:trips
      (trip ^ copy)
      [ plan [ at "jobs.0.outputs.output.path" {|"output/album/trip=alpha"|} ] ];
    test "a_source_may_sit_in_a_source_folder" ~header ~files:trips
      (trip ^ "source note: Text .txt [trip]\npath note: input/{trip}/evening.txt\n" ^ copy
     ^ "operation read(note: Text) -> Text\n\
        command read: cat {note} > {@output}\n\
        read_note = read(note)\n")
      [
        plan
          [ at "external_inputs.*.path" {|["input/alpha","input/alpha/evening.txt","input/beta"]|} ];
      ];
    test "nothing_is_written_in_a_folder_a_job_writes" ~header ~files:trips
      (trip ^ copy
     ^ "operation count(album: Album) -> Count .txt\n\
        command count: ls {album} > {@output}\n\
        counted = count(album)\n\
        path counted: output/album/{@entities}/count\n")
      [
        plan
          [
            problem
              (Inside_output_folder
                 {
                   product = "counted";
                   folder = "album";
                   path = "output/album/trip=alpha/count.txt";
                 });
          ];
      ];
    test "a_seeded_folder_names_its_input_port_in_the_dag" ~header:""
      ~files:[ "in/dune.txt"; "in/kelp.txt" ] (bundle ^ catalogue)
      [
        plan
          [
            at "jobs.1.seed_folders" {|{"output":"bundle"}|};
            at "jobs.1.outputs.output.kind" {|"folder"|};
            at "jobs.0.commands.0.command.3" {|[{"path":"out/bundled"},"/"]|};
          ];
      ];
    test "a_seed_names_one_folder_input" ~header:""
      (bundle ^ replace "from bundle" "from missing" catalogue)
      [ check [ problem (Seed_port { operation = "catalogue"; port = "missing" }) ] ];
    test "a_seed_is_not_a_file_input" ~header:""
      (bundle
     ^ "operation catalogue(bundle: Bundle, mean: Mean) -> Catalogue / from mean\n\
        command catalogue: ls {@output} {mean}\n\
        catalogued = catalogue(bundled, mean @same(site))\n")
      [ check [ problem (Seed_port { operation = "catalogue"; port = "mean" }) ] ];
    viewed;
    test "a_view_operation_has_no_command" ~header:""
      (pack ^ "command pack: echo {mean} {card}\n")
      [ check [ problem (View_command "pack") ] ];
    test "a_view_member_lies_inside_the_view" ~header:"" ~files:[ "in/dune.txt"; "in/dune.card" ]
      (replace "path labelled: pack/{site}/card.txt" "path labelled: cards/{site}.txt" pack)
      [
        plan
          [
            problem
              (View_member_outside { view = "packed"; member = "labelled"; path = "cards/dune.txt" });
          ];
      ];
  ]
