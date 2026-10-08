//! A placeholder in a source path rule may take a shape, as `{date:date}`,
//! so only names of that shape are read: how it matches, where it is
//! allowed, and what a file that fails it looks like.

mod support;

use std::process::{Command, Output};

use support::{text, Tree};

const LOGS: [&str; 12] = [
    "logs/web1/2026-09-01.log",
    "logs/web1/2026-09-02.log",
    "logs/web2/2026-09-01.log",
    "logs/web1/notes.log",
    "logs/web2/readme.log",
    "logs/web1/2026-9-1.log",
    "logs/web1/20260901.log",
    "logs/web1/2026-09-01-final.log",
    "logs/web1/2026_09_02.log",
    "logs/web1/2026-02-30.log",
    "logs/web1/2026-09-01.log.gz",
    "logs/README.txt",
];

fn run(tree: &Tree, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .current_dir(tree.path())
        .output()
        .unwrap()
}

fn logs(rule: &str) -> Tree {
    let tree = Tree::new("shapes-logs", &LOGS);
    tree.write(
        "a.spit",
        &format!("source log [server, date]\npath log: {rule}\n"),
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    tree
}

/// The artifacts `inputs` found, as `server date`.
fn found(tree: &Tree) -> Vec<String> {
    let output = run(tree, &["inputs", "a.spitin"]);
    let stdout = text(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("log[server="))
        .map(|line| line.trim_end_matches(']').replace(",date=", " "))
        .collect()
}

#[test]
fn a_rule_without_a_shape_reads_every_name_as_a_date() {
    let tree = logs("logs/{server}/{date}.log");
    let found = found(&tree);
    assert!(found.contains(&"web1 notes".to_owned()), "{found:?}");
    assert!(found.contains(&"web1 2026-09-01-final".to_owned()));
}

#[test]
fn a_date_shape_reads_only_real_dates() {
    let tree = logs("logs/{server}/{date:date}.log");
    assert_eq!(
        found(&tree),
        ["web1 2026-09-01", "web1 2026-09-02", "web2 2026-09-01"]
    );
}

#[test]
fn a_file_failing_the_shape_is_counted_as_unmatched() {
    let tree = logs("logs/{server}/{date:date}.log");
    let output = run(&tree, &["inputs", "a.spitin"]);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("note: 9 files under `.` match no source rule"),
        "{stderr}"
    );
    let listed = text(&run(&tree, &["inputs", "a.spitin", "--unmatched"]).stdout);
    for stray in [
        "logs/web1/notes.log",
        "logs/web1/2026-9-1.log",
        "logs/web1/20260901.log",
        "logs/web1/2026-09-01-final.log",
        "logs/web1/2026-02-30.log",
    ] {
        assert!(
            listed.lines().any(|line| line == stray),
            "{stray}: {listed}"
        );
    }
}

#[test]
fn the_nearest_file_shows_the_rule_with_its_shape() {
    let tree = Tree::new("shapes-nearest", &["logs/web1/notes.log"]);
    tree.write(
        "a.spit",
        "source log [server, date]\npath log: logs/{server}/{date:date}.log\n\
         operation d(log) -> .json\ncommand d: d {log} {@output}\nout = d(log)\n",
    );
    let output = run(&tree, &["dag", "a.spit", "--root", "."]);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("matched no files with path rule `logs/{server}/{date:date}.log`"),
        "{stderr}"
    );
    assert!(
        stderr.contains(
            "after `logs/web1/`, the file has `notes.log` where the rule has `{date:date}.log`"
        ),
        "{stderr}"
    );
}

#[test]
fn adjacent_placeholders_split_where_a_fixed_shape_ends() {
    let tree = Tree::new(
        "shapes-adjacent",
        &["in/2026-09-01-3.txt", "in/2026-09-01-x.txt"],
    );
    tree.write(
        "a.spit",
        "source s [date, run]\npath s: in/{date:date}-{run:digits}.txt\n",
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("s[date=2026-09-01,run=3]"), "{stdout}");
    assert!(!stdout.contains("run=x"), "{stdout}");
}

#[test]
fn a_year_then_digits_need_no_separator() {
    let tree = Tree::new("shapes-year", &["in/202401.txt", "in/2124.txt"]);
    tree.write(
        "a.spit",
        "source s [year, n]\npath s: in/{year:year}{n:digits}.txt\n",
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("s[year=2024,n=01]"), "{stdout}");
    assert!(!stdout.contains("2124"), "{stdout}");
}

#[test]
fn a_shape_applies_to_part_of_a_name() {
    let tree = Tree::new(
        "shapes-part",
        &["sub-01_T1w.nii", "sub-ab_T1w.nii", "sub-01-02_T1w.nii"],
    );
    tree.write(
        "a.spit",
        "source s [sub]\npath s: sub-{sub:digits}_T1w.nii\n",
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("s[sub=01]"), "{stdout}");
    assert!(
        !stdout.contains("sub=ab") && !stdout.contains("01-02"),
        "{stdout}"
    );
}

#[test]
fn a_dimension_written_twice_keeps_its_shape_in_both_places() {
    let tree = Tree::new("shapes-twice", &["7/7.txt", "a/a.txt", "7/8.txt"]);
    tree.write("a.spit", "source s [id]\npath s: {id:digits}/{id}.txt\n");
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("s[id=7]"), "{stdout}");
    assert!(
        !stdout.contains("id=a") && !stdout.contains("id=8"),
        "{stdout}"
    );
}

#[test]
fn a_leading_zero_is_kept() {
    let tree = Tree::new("shapes-zero", &["r/run-007.txt"]);
    tree.write("a.spit", "source s [run]\npath s: r/run-{run:digits}.txt\n");
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    assert!(text(&run(&tree, &["inputs", "a.spitin"]).stdout).contains("s[run=007]"));
}

#[test]
fn two_rules_that_both_read_a_file_still_conflict() {
    let tree = Tree::new("shapes-overlap", &["logs/2026-09-01.log"]);
    tree.write(
        "a.spit",
        "source dated [date]\npath dated: logs/{date:date}.log\n\
         source named [name]\npath named: logs/{name}.log\n",
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stderr = text(&run(&tree, &["inputs", "a.spitin"]).stderr);
    assert!(
        stderr.contains("matches the path rules of both `dated` and `named`"),
        "{stderr}"
    );
}

#[test]
fn a_recipe_rule_and_a_discover_rule_take_shapes() {
    let tree = Tree::new(
        "shapes-recipe",
        &[
            "data/2026-09-01/a.txt",
            "data/old/b.txt",
            "data/2026-09-02/a.txt",
        ],
    );
    tree.write("a.spit", "source s [day]\n");
    tree.write(
        "a.spitin",
        "pipeline a.spit\nroot .\ndiscover days: [day] from dirs data/{day:date}\npath s: data/{day:date}/a.txt\n",
    );
    let output = run(&tree, &["inputs", "a.spitin"]);
    let stdout = text(&output.stdout);
    assert!(stdout.contains("[day=2026-09-01]") && stdout.contains("[day=2026-09-02]"));
    assert!(stdout.contains("s: data/{day:date}/a.txt"), "{stdout}");
    assert!(stdout.contains("contexts days:"), "{stdout}");
    assert!(!stdout.contains("old"), "{stdout}");
}

fn check(source: &str) -> String {
    let tree = Tree::new("shapes-check", &[]);
    tree.write("a.spit", source);
    let output = run(&tree, &["check", "a.spit"]);
    format!("{}{}", text(&output.stdout), text(&output.stderr))
}

#[test]
fn a_bad_shape_says_which_shapes_there_are() {
    let said = check("source s [d]\npath s: in/{d:dat}.txt\n");
    assert!(
        said.contains(
            "unknown shape `dat` in `{d:dat}`; the shapes are `digits`, `year` and `date`"
        ),
        "{said}"
    );
}

#[test]
fn a_built_in_placeholder_takes_no_shape() {
    let said = check("source s [d]\npath s: in/{@entities:digits}.txt\n");
    assert!(
        said.contains("a built-in placeholder takes no shape"),
        "{said}"
    );
}

#[test]
fn one_dimension_has_one_shape() {
    let said = check("source s [d]\npath s: in/{d:date}/{d:year}.txt\n");
    assert!(
        said.contains("`d` has two shapes, `date` and `year`"),
        "{said}"
    );
}

#[test]
fn two_open_shapes_need_text_between() {
    let said = check("source s [a, b]\npath s: in/{a:digits}{b:digits}.txt\n");
    assert!(
        said.contains("no text between two shapes of any length"),
        "{said}"
    );
}

#[test]
fn a_shape_on_an_output_or_a_default_is_an_error() {
    let output = check(
        "source s [d]\npath s: in/{d}.txt\noperation f(s) -> .txt\ncommand f: f {s} {@output}\n\
         o = f(s)\npath o: out/{d:digits}.txt\n",
    );
    assert!(
        output.contains("shapes narrow a source's path rule only; `o` is made by a step"),
        "{output}"
    );
    let default = check(
        "source s [d]\npath s: in/{d}.txt\npath: out/{@product}/{d:digits}\n\
         operation f(s) -> .txt\ncommand f: f {s} {@output}\no = f(s)\n",
    );
    assert!(
        default.contains("`path:` is the default for outputs too"),
        "{default}"
    );
}

#[test]
fn a_record_whose_value_fails_its_rules_shape_is_rejected() {
    let tree = Tree::new("shapes-record", &["logs/web1/notes.log"]);
    tree.write(
        "a.spit",
        "source log [server, date]\npath log: logs/{server}/{date:date}.log\n\
         operation d(log) -> .json\ncommand d: d {log} {@output}\nout = d(log)\n",
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    tree.write(
        "a.spitout",
        "root .\n\nsources:\n    log[server=web1,date=notes]\n",
    );
    let output = run(&tree, &["dag", "a.spit", "a.spitout"]);
    let said = format!("{}{}", text(&output.stdout), text(&output.stderr));
    assert!(
        said.contains("`date=notes`, which is not a `date`"),
        "{said}"
    );
}

#[test]
fn two_sources_with_one_dimension_name_collide_whatever_their_shapes() {
    let said =
        check("source a [d]\nsource b [d]\npath a: in/{d:date}.log\npath b: in/{d:digits}.log\n");
    assert!(
        said.contains(
            "products `a` and `b` bind to the same path `in/d.log` for the same entities"
        ),
        "{said}"
    );
    let named =
        check("source a [d]\nsource b [n]\npath a: in/{d:date}.log\npath b: in/{n:digits}.log\n");
    assert!(named.contains("Pipeline valid."), "{named}");
}

#[test]
fn the_default_owner_error_names_the_rule_to_write() {
    let said = check(
        "source s [d]\npath: in/{d:digits}.txt\noperation f(s) -> .txt\ncommand f: f {s} {@output}\no = f(s)\n",
    );
    assert!(
        said.contains("write the shape in a `path` rule for each source, as `path <source>: ...`"),
        "{said}"
    );
    assert!(!said.contains("path source:"), "{said}");
    let fixed =
        check("source s [d]\npath: out/{@product}/{@entities}\npath s: in/{d:digits}.txt\n");
    assert!(fixed.contains("Pipeline valid."), "{fixed}");
}

#[test]
fn a_dropped_group_can_leave_two_open_shapes_touching() {
    let tree = Tree::new("shapes-dropped", &["in/12.txt", "in/1-x2.txt"]);
    tree.write("a.spit", "source s [a, b]\nsource t [a, b, c]\n");
    tree.write(
        "a.spitin",
        "pipeline a.spit\nroot .\npath: in/{a:digits}[-{c}]{b:digits}.txt\n",
    );
    let said = text(&run(&tree, &["check", "a.spitin"]).stderr);
    assert!(
        said.contains("in the path rule for `s`, once the groups its dimensions lack are dropped"),
        "{said}"
    );
    assert!(
        said.contains("no text between two shapes of any length"),
        "{said}"
    );
    // The product with `c` keeps its group, so it is not named.
    assert!(!said.contains("rule for `t`"), "{said}");
}

#[test]
fn a_dropped_group_leaving_a_separator_is_fine() {
    let tree = Tree::new("shapes-kept", &[]);
    tree.write("a.spit", "source s [a, b]\nsource t [a, b, c]\n");
    tree.write(
        "a.spitin",
        "pipeline a.spit\nroot .\npath: in/{a:digits}-[{c}-]{b:digits}.txt\n",
    );
    let said = text(&run(&tree, &["check", "a.spitin"]).stdout);
    assert!(said.contains("Recipe valid."), "{said}");
}

#[test]
fn a_shape_beside_an_unshaped_placeholder_splits_shortest_first() {
    let tree = Tree::new("shapes-mixed", &["in/123.txt", "in/1x.txt"]);
    tree.write("a.spit", "source s [a, b]\npath s: in/{a:digits}{b}.txt\n");
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("s[a=1,b=23]"), "{stdout}");
    assert!(stdout.contains("s[a=1,b=x]"), "{stdout}");
}

#[test]
fn a_date_on_a_leap_day_follows_the_century_rule() {
    let files = [
        "in/1900-02-29.log",
        "in/2000-02-29.log",
        "in/2024-02-29.log",
        "in/2100-02-29.log",
        "in/1899-01-01.log",
        "in/2100-01-01.log",
        "in/2023-02-29.log",
    ];
    let tree = Tree::new("shapes-century", &files);
    tree.write("a.spit", "source s [d]\npath s: in/{d:date}.log\n");
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    let found: Vec<_> = stdout
        .lines()
        .filter(|line| line.contains("s[d="))
        .collect();
    assert_eq!(found.len(), 2, "{stdout}");
    assert!(stdout.contains("s[d=2000-02-29]") && stdout.contains("s[d=2024-02-29]"));
}

#[test]
fn a_folder_source_takes_a_shape() {
    let tree = Tree::new(
        "shapes-folder",
        &[
            "runs/2024/a.txt",
            "runs/2025/a.txt",
            "runs/abc/a.txt",
            "runs/2124/a.txt",
        ],
    );
    tree.write("a.spit", "source s : T / [y]\npath s: runs/{y:year}\n");
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(
        stdout.contains("s[y=2024]") && stdout.contains("s[y=2025]"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("abc") && !stdout.contains("2124"),
        "{stdout}"
    );
}

#[test]
fn a_beside_companion_inherits_the_shape() {
    let tree = Tree::new(
        "shapes-beside",
        &[
            "site-a/2024-01-05.raw",
            "site-a/2024-01-05.json",
            "site-a/notes.raw",
            "site-a/notes.json",
        ],
    );
    tree.write(
        "a.spit",
        "source raw .raw [site, d]\npath raw: site-{site}/{d:date}.raw\nsource meta .json beside raw\n",
    );
    tree.write("a.spitin", "pipeline a.spit\nroot .\n");
    let rules = text(&run(&tree, &["check", "a.spit", "--path-rules"]).stdout);
    assert!(
        rules.contains("beside raw site-{site}/{d:date}.json"),
        "{rules}"
    );
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("raw[site=a,d=2024-01-05]"), "{stdout}");
    assert!(stdout.contains("meta[site=a,d=2024-01-05]"), "{stdout}");
    assert!(!stdout.contains("notes"), "{stdout}");
}

#[test]
fn a_shape_inside_an_optional_group_applies_where_the_group_is_kept() {
    let tree = Tree::new(
        "shapes-group",
        &[
            "in/2024-01-05.txt",
            "in/2024-01-05-2.txt",
            "in/2024-01-06-xx.txt",
            "in/2024-01-07-3.txt",
        ],
    );
    tree.write("a.spit", "source s [d]\nsource t [d, v]\n");
    tree.write(
        "a.spitin",
        "pipeline a.spit\nroot .\npath s: in/{d:date}.txt\npath t: in/{d:date}[-{v:digits}].txt\n",
    );
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(stdout.contains("t[d=2024-01-05,v=2]"), "{stdout}");
    assert!(stdout.contains("t[d=2024-01-07,v=3]"), "{stdout}");
    assert!(!stdout.contains("xx"), "{stdout}");
    assert!(stdout.contains("s[d=2024-01-05]"), "{stdout}");
}

#[test]
fn a_recipes_shared_default_takes_a_shape() {
    let tree = Tree::new(
        "shapes-recipe-default",
        &["in/2024-01-05.txt", "in/2024-01-06.txt", "in/notes.txt"],
    );
    tree.write("p.spit", "source s [d]\n");
    tree.write(
        "a.spitin",
        "pipeline p.spit\nroot .\npath: in/{d:date}.txt\n",
    );
    let stdout = text(&run(&tree, &["inputs", "a.spitin"]).stdout);
    assert!(
        stdout.contains("s[d=2024-01-05]") && stdout.contains("s[d=2024-01-06]"),
        "{stdout}"
    );
    assert!(!stdout.contains("notes"), "{stdout}");
}
