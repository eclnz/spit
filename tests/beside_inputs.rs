//! An output written beside one of its job's inputs, such as the `.fai`
//! samtools writes next to the FASTA it indexes, lies beside that input's
//! file, source or output; and a `+` suffix follows a file's whole name.

mod support;

use spit::{diagnose, parse_pipeline};
use support::{spit, text, Tree};

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .collect()
}

/// `spit dag --paths` on `pipeline`, written in a tree holding `files`,
/// with `recipe` beside it when given: its output, or its error.
fn dag_paths(files: &[&str], pipeline: &str, recipe: Option<&str>) -> Result<String, String> {
    let tree = Tree::new("beside-inputs", files);
    let pipeline = tree.write("p.spit", pipeline);
    // A recipe names its pipeline.
    let args = match recipe {
        Some(recipe) => vec![tree.write("p.spitin", recipe)],
        None => vec![pipeline],
    };
    let mut command: Vec<&str> = vec!["dag"];
    command.extend(args.iter().map(|path| path.to_str().unwrap()));
    command.push("--paths");
    let output = spit(&command);
    match output.status.success() {
        true => Ok(text(&output.stdout)),
        false => Err(text(&output.stderr)),
    }
}

const FAIDX: &str = "\
root data
source reference : Fasta .fasta
source bam : Bam .bam [sample]
path bam: reads/{sample}
path pileups: out/{sample}
operation faidx(fasta: Fasta) -> Fai +.fai beside fasta
command faidx: samtools faidx {fasta}
genome_fai = faidx(reference)
operation pileup(fasta: Fasta, fai: Fai, bam: Bam) -> Bcf .bcf
command pileup: bcftools mpileup -f {fasta} -o {@output} {bam}
pileups = pileup(reference, genome_fai, bam)
";

const FILES: [&str; 2] = ["data/reference/genome.fasta", "data/reads/a.bam"];

#[test]
fn an_output_beside_a_source_lies_beside_its_file_and_its_reader_waits_for_it() {
    let pipeline = format!("{FAIDX}path reference: reference/genome\n");
    let paths = dag_paths(&FILES, &pipeline, None).unwrap();
    assert!(
        paths.contains("  output:\n    genome_fai : Fai\n      path: reference/genome.fasta.fai\n"),
        "{paths}"
    );
    // The reader names it as an input, though its command does not.
    assert!(
        paths.contains("    fai: genome_fai : Fai\n      path: reference/genome.fasta.fai\n"),
        "{paths}"
    );
    assert!(paths.contains("  depends_on: 1\n"), "{paths}");
}

#[test]
fn an_output_beside_a_source_follows_the_rule_a_recipe_gives_it() {
    let recipe = "pipeline p.spit\ninputs:\n    reference: reference/genome.fasta\n";
    let paths = dag_paths(&FILES, FAIDX, Some(recipe)).unwrap();
    assert!(
        paths.contains("path: reference/genome.fasta.fai\n"),
        "{paths}"
    );
    // The pipeline alone checks, and says where the file lies.
    let tree = Tree::new("beside-inputs-listing", &[]);
    let file = tree.write("p.spit", FAIDX);
    let output = spit(&["check", file.to_str().unwrap(), "--path-rules"]);
    let listing = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        listing.contains("  genome_fai (output): beside reference +.fai\n"),
        "{listing}"
    );
}

#[test]
fn an_output_beside_an_input_takes_the_value_the_step_pins() {
    let pipeline = parse_pipeline(
        "source reference : Fasta .fasta [build]\npath reference: ref/{build}/genome\n\
         operation faidx(fasta: Fasta) -> Fai +.fai beside fasta\ncommand faidx: samtools faidx {fasta}\n\
         fai = faidx(reference @where(build=hg38))\n",
    )
    .unwrap();
    assert_eq!(
        pipeline.path_template_for("fai").unwrap().to_string(),
        "ref/hg38/genome.fasta.fai"
    );
}

#[test]
fn a_plain_suffix_beside_an_input_replaces_its_extension() {
    let pipeline = parse_pipeline(
        "source image : Image .nii.gz [sub]\npath image: sub-{sub}_T1w\n\
         operation stats(image: Image) -> Json .json beside image\ncommand stats: stats {image}\n\
         s = stats(image)\n",
    )
    .unwrap();
    assert_eq!(
        pipeline.path_template_for("s").unwrap().to_string(),
        "sub-{sub}_T1w.json"
    );
}

#[test]
fn an_output_beside_an_output_of_another_step_lies_in_its_folder() {
    let pipeline = parse_pipeline(
        "default path: {@stage}/{@product}/{sub}\nsource bam : Bam .bam [sub]\npath bam: in/{sub}\n\
         operation sort(bam: Bam) -> Bam .bam\ncommand sort: samtools sort -o {@output} {bam}\n\
         operation index(bam: Bam) -> Bai +.bai beside bam\ncommand index: samtools index {bam}\n\
         stage align:\n    sorted = sort(bam)\nstage qc:\n    bai = index(sorted)\n",
    )
    .unwrap();
    assert_eq!(
        pipeline.path_template_for("bai").unwrap().to_string(),
        "align/sorted/{sub}.bam.bai"
    );
}

#[test]
fn a_plus_suffix_follows_the_whole_name_in_each_place_beside_is_written() {
    let pipeline = parse_pipeline(
        "source reference : Fasta .fasta\npath reference: ref/genome\n\
         source dict : Dict +\"_dict.txt\" beside reference\n\
         source sam : Sam .sam [sub]\npath sam: in/{sub}\n\
         default path: out/{@product}/{sub}\n\
         operation sort(sam: Sam) -> (bam: Bam .bam, bai: Bai +.bai beside bam)\n\
         command sort: samtools sort -o {bam} {sam}\nsorted, sorted_bai = sort(sam)\n",
    )
    .unwrap();
    for (product, path) in [
        ("dict", "ref/genome.fasta_dict.txt"),
        ("sorted_bai", "out/sorted/{sub}.bam.bai"),
    ] {
        assert_eq!(
            pipeline.path_template_for(product).unwrap().to_string(),
            path
        );
    }
    let beside = pipeline.beside("sorted_bai").unwrap();
    assert!(beside.append);
    assert_eq!((beside.sibling, beside.suffix), ("sorted", ".bai"));
}

#[test]
fn the_input_an_output_is_written_beside_gives_each_job_its_own_file() {
    let head = "source reference : Fasta .fasta\npath reference: ref/genome\n\
                source bam : Bam .bam [sample, lane]\npath bam: in/{sample}_{lane}\n\
                source other : Bam .bam [sample, lane]\npath other: in2/{sample}_{lane}\n\
                default path: out/{@product}/{sample}[_{lane}]\n";
    for (step, message) in [
        (
            "operation f(fasta: Fasta, bam: Bam) -> Fai +.fai beside fasta\ncommand f: t {fasta} {bam}\nx = f(reference, bam)\n",
            "output `output` is written beside input `fasta`, so each job must read its own `fasta` file, but `reference` has dimensions [], and the step's jobs [sample, lane]",
        ),
        (
            "operation f(bams: many Bam) -> Fai +.fai beside bams\ncommand f: t {bams}\nx = f(bam @vary(lane))\n",
            "`output` is written beside `bams`, a `many` input",
        ),
        (
            "operation f(a: Bam, b: Bam) -> Bai +.bai beside b\ncommand f: t {a} {b}\nx = f(other, bam @same(sample))\n",
            "output `output` is written beside input `b`, so each job must read its own `b` file, but `bam` is matched on [sample] only, of the step's [sample, lane]",
        ),
    ] {
        let found = errors(&format!("{head}{step}"));
        assert!(
            found.iter().any(|error| error.contains(message)),
            "{step}: {found:?}"
        );
    }
}

#[test]
fn an_output_lies_only_beside_a_file_with_the_extension_it_replaces() {
    for (text, message) in [
        (
            "source dicom : Dicom / [sub]\npath dicom: in/{sub}\ndefault path: out/{@product}/{sub}\n\
             operation f(d: Dicom) -> Json +.json beside d\ncommand f: t {d}\nx = f(dicom)\n",
            "`x` is written beside `dicom`, which is a folder, and only a file has files beside it",
        ),
        (
            "source reference : Fasta\noperation f(fasta: Fasta) -> Fai .fai beside fasta\n\
             command f: t {fasta}\nx = f(reference)\n",
            "`x` is written beside `reference`, which declares no extension for `.fai` to replace; declare one, as in `source reference : Type .ext`, or append to its whole name with `+.fai`",
        ),
    ] {
        let found = errors(text);
        assert!(
            found.iter().any(|error| error.contains(message)),
            "{text}: {found:?}"
        );
    }
}

#[test]
fn an_output_beside_a_source_is_neither_a_source_nor_an_unmatched_file() {
    // The `log` rule would read `reads/a_qc.log` as a log named `a_qc`.
    let pipeline = "root data\nsource bam : Bam .bam [sample]\npath bam: reads/{sample}\n\
                    source log : Log .log [name]\npath log: reads/{name}\n\
                    operation index(bam: Bam) -> (bai: Bai +.bai beside bam, qc: Log \"_qc.log\" beside bam)\n\
                    command index: samtools index {bam}\nbai, qc = index(bam)\n\
                    operation keep(log: Log) -> Log .log\ncommand keep: cp {log} {@output}\n\
                    path kept: out/{name}\nkept = keep(log)\n";
    let tree = Tree::new(
        "beside-inputs-discovery",
        &[
            "data/reads/a.bam",
            "data/reads/a.bam.bai",
            "data/reads/a_qc.log",
            "data/reads/run.log",
        ],
    );
    let file = tree.write("p.spit", pipeline);
    let output = spit(&["inputs", file.to_str().unwrap()]);
    let (found, notes) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{notes}");
    assert_eq!(
        found, "sources:\n    bam[sample=a]\n    log[name=run]\n",
        "{notes}"
    );
    assert!(!notes.contains("match no source rule"), "{notes}");
}

#[test]
fn an_output_cannot_be_written_inside_a_source_folder() {
    let pipeline = "root data\nsource scans : Dicom / [sub]\npath scans: scans/{sub}\n\
                    source first : Image .nii [sub]\npath first: scans/{sub}/first\n\
                    operation f(image: Image, scans: Dicom) -> Json +.json beside image\n\
                    command f: t {image} {scans}\nx = f(first, scans)\n";
    let error = dag_paths(&["data/scans/a/first.nii"], pipeline, None).unwrap_err();
    assert!(
        error.contains("source folder, which no job may write inside"),
        "{error}"
    );
}
