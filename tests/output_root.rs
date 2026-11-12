//! An output root apart from the dataset root: where it is declared, which
//! artifacts are under it, and what the `.spitdag` and `.spitout` record.

mod support;

use support::{spit, text, Tree};

/// A FASTA with an index written beside it, and calls written from it.
const PIPELINE: &str = "\
source reference : Fasta .fasta
source bam : Bam .bam [sample]
path reference: reference/genome
path bam: reads/{sample}
path pileups: calls/{sample}
operation faidx(fasta: Fasta) -> Fai +.fai beside fasta
command faidx: samtools faidx {fasta}
genome_fai = faidx(reference)
operation pileup(fasta: Fasta, fai: Fai, bam: Bam) -> Bcf .bcf
command pileup: bcftools mpileup -f {fasta} -o {@output} {bam}
pileups = pileup(reference, genome_fai, bam)
";

fn dataset(recipe: &str) -> (Tree, String) {
    let tree = Tree::new(
        "output-root",
        &["data/reference/genome.fasta", "data/reads/A.bam"],
    );
    tree.write("analysis.spit", PIPELINE);
    let recipe = tree.write("dataset.spitin", recipe);
    let recipe = recipe.to_str().unwrap().to_owned();
    (tree, recipe)
}

fn succeeded(args: &[&str]) -> String {
    let output = spit(args);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn outputs_go_under_the_output_root_and_files_beside_a_source_stay_with_it() {
    let (tree, recipe) = dataset("pipeline analysis.spit\nroot data\noutput results\n");
    let plan = succeeded(&["dag", &recipe, "--json"]);
    assert!(plan.starts_with("{\"version\":13,"), "{plan}");
    let folder = tree.path().display().to_string();
    assert!(
        plan.contains(&format!(
            "\"root\":\"{folder}/data\",\"output_root\":\"{folder}/results\","
        )),
        "{plan}"
    );
    for (path, under) in [
        ("reference/genome.fasta", "root"),
        ("reference/genome.fasta.fai", "root"),
        ("reads/A.bam", "root"),
        ("calls/A.bcf", "output_root"),
    ] {
        assert!(
            plan.contains(&format!("\"path\":\"{path}\",\"under\":\"{under}\"")),
            "{path}: {plan}"
        );
    }
}

#[test]
fn without_an_output_root_outputs_are_under_the_dataset_root() {
    let (tree, recipe) = dataset("pipeline analysis.spit\nroot data\n");
    let plan = succeeded(&["dag", &recipe, "--json"]);
    let data = tree.path().join("data").display().to_string();
    assert!(
        plan.contains(&format!("\"root\":\"{data}\",\"output_root\":\"{data}\",")),
        "{plan}"
    );
    assert!(plan.contains("\"path\":\"calls/A.bcf\",\"under\":\"output_root\""));
}

#[test]
fn commands_run_from_the_output_root_and_reach_the_dataset_from_it() {
    let (_tree, recipe) = dataset("pipeline analysis.spit\nroot data\noutput results\n");
    let output = spit(&["dag", &recipe, "--commands"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("note: commands run from `"));
    assert!(text(&output.stderr).contains("results`"));
    let commands = text(&output.stdout);
    assert!(
        commands.contains("run:    samtools faidx ../data/reference/genome.fasta\n"),
        "{commands}"
    );
    assert!(
        commands.contains(
            "run:    bcftools mpileup -f ../data/reference/genome.fasta -o calls/A.bcf ../data/reads/A.bam\n"
        ),
        "{commands}"
    );
}

#[test]
fn a_written_spitout_records_the_output_root_beside_the_dataset_root() {
    let (tree, recipe) = dataset("pipeline analysis.spit\nroot data\noutput results\n");
    let spitout = tree.path().join("records/dataset.spitout");
    std::fs::create_dir_all(spitout.parent().unwrap()).unwrap();
    let spitout = spitout.to_str().unwrap();
    succeeded(&["inputs", &recipe, "-o", spitout]);
    let records = std::fs::read_to_string(spitout).unwrap();
    assert!(
        records.starts_with("root ../data\noutput ../results\n\nsources:\n"),
        "{records}"
    );
    let pipeline = tree.path().join("analysis.spit");
    let plan = succeeded(&["dag", pipeline.to_str().unwrap(), spitout, "--json"]);
    assert!(plan.contains("/results\",\"external_inputs\""), "{plan}");
}

#[test]
fn a_pipeline_names_its_output_root_or_takes_it_from_the_command_line() {
    let (tree, _) = dataset("");
    let alone = tree.write(
        "alone.spit",
        &format!("root data\noutput results\n{PIPELINE}"),
    );
    let plan = succeeded(&["dag", alone.to_str().unwrap(), "--json"]);
    assert!(plan.contains("/results\",\"external_inputs\""), "{plan}");

    let pipeline = tree.path().join("analysis.spit");
    let root = tree.path().join("data");
    let given = tree.path().join("given");
    let plan = succeeded(&[
        "dag",
        pipeline.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--output-root",
        given.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.contains("/given\",\"external_inputs\""), "{plan}");
}

#[test]
fn an_output_root_is_declared_once() {
    let (tree, _) = dataset("");
    let both = tree.write("both.spit", &format!("output elsewhere\n{PIPELINE}"));
    let recipe = tree.write(
        "both.spitin",
        "pipeline both.spit\nroot data\noutput results\n",
    );
    let output = spit(&["dag", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr)
            .contains("output root is declared in both .spit and .spitin; keep one declaration"),
        "{}",
        text(&output.stderr)
    );

    let flagged = spit(&[
        "dag",
        both.to_str().unwrap(),
        "--root",
        "data",
        "--output-root",
        "x",
    ]);
    assert!(text(&flagged.stderr).contains(
        "output root is declared in both .spit and `--output-root`; keep one declaration"
    ));

    let (_tree, recipe) = dataset("pipeline analysis.spit\nroot data\n");
    let output = spit(&["dag", &recipe, "--output-root", "x"]);
    assert!(text(&output.stderr).contains(
        "names where outputs go with an `output` line; `--output-root` is for a pipeline run without a recipe"
    ));
}

#[test]
fn output_is_a_keyword_no_product_may_take() {
    let (tree, _) = dataset("");
    let pipeline = tree.write(
        "named.spit",
        "source raw [id]\noperation f(x) -> Y\noutput = f(raw)\n",
    );
    let output = spit(&["check", pipeline.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("`output` is one of SPIT's keywords"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_scan_leaves_out_an_output_root_inside_the_dataset() {
    let tree = Tree::new(
        "output-root-inside",
        &[
            "reference/genome.fasta",
            "reads/A.bam",
            "notes.txt",
            "results/calls/A.bcf",
            "results/run.log",
        ],
    );
    let pipeline = tree.write(
        "analysis.spit",
        &format!("root .\noutput results\n{PIPELINE}"),
    );
    let unmatched = succeeded(&["inputs", pipeline.to_str().unwrap(), "--unmatched"]);
    assert_eq!(unmatched, "notes.txt\n");
}
