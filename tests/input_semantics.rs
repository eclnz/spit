//! Input matching and requirements are independent: paths observe, rules oblige.

mod support;

use std::fs;

use spit::{
    parse_input_spec, parse_pipeline, parse_source_inventory, render_source_inventory, InputSource,
};
use support::Tree;

#[test]
fn input_block_checks_its_structure_and_duplicate_paths() {
    for (text, line) in [
        ("inputs:\n", 1),
        ("inputs:\nrequire [] where lut count=1\n", 1),
        ("inputs:\n    lut: lut.tsv\n      reads: reads.fq\n", 3),
        (
            "inputs:\n    lut: lut.tsv\ninputs:\n    reads: reads.fq\n",
            3,
        ),
        ("inputs:\n    lut: lut.tsv\n    lut: other.tsv\n", 3),
        (
            "inputs:\n    default: a/{@product}\n    default: b/{@product}\n",
            3,
        ),
    ] {
        assert_eq!(parse_input_spec(text).expect_err(text).line(), line);
    }
    assert!(parse_input_spec(
        "inputs:\n    # source paths\n\n    lut: lut.tsv\n\nrequire [] where lut count=1\n"
    )
    .is_ok());
}

#[test]
fn matching_paths_do_not_make_optional_scene_products_mandatory() {
    let tree = Tree::new(
        "scene-products",
        &["scenes/A/optical.tif", "scenes/B/radar.tif"],
    );
    let pipeline = parse_pipeline(
        "source optical [scene]\nsource radar [scene]\nsource temperature [scene]\n",
    )
    .expect("pipeline");
    let recipe = parse_input_spec("inputs:\n    optical: scenes/{scene}/optical.tif\n    radar: scenes/{scene}/radar.tif\n    temperature: scenes/{scene}/temperature.tif\n    contexts: scenes/{scene}\n").expect("recipe");
    let result = recipe
        .resolve(&pipeline, InputSource::Discover(&tree.0))
        .expect("scan");
    assert_eq!(result.inventory.artifacts.len(), 2);
    assert!(result.gaps.is_empty());
}

#[test]
fn removed_recipe_forms_are_rejected() {
    for text in [
        "discover sessions: [sample] from dirs samples/{sample}\n",
        "path reads: {sample}.fq\n",
        "path: {@product}/{@entities}\n",
    ] {
        assert!(parse_input_spec(text).is_err(), "{text}");
    }
    assert!(parse_source_inventory("contexts sessions:\n    [sample=A]\n").is_err());
}

#[test]
fn a_literal_directory_can_establish_a_global_context() {
    let tree = Tree::new("global-context", &[]);
    fs::create_dir_all(tree.0.join("reference")).expect("folder");
    let pipeline = parse_pipeline("source lut\n").expect("pipeline");
    let recipe = parse_input_spec("inputs:\n    lut: reference/lut.tsv\n    contexts: reference\nrequire [] where contexts[] count=1\nrequire [] where lut count=1\n").expect("recipe");
    let result = recipe
        .resolve(&pipeline, InputSource::Discover(&tree.0))
        .expect("scan");
    assert_eq!(result.gaps.len(), 1);
    assert!(result.gaps[0].error.to_string().contains("lut"));
}

#[test]
fn paired_reads_require_the_missing_mate() {
    let tree = Tree::new("paired-reads", &["fastq/A_L1_R1.fastq.gz"]);
    let pipeline =
        parse_pipeline("source r1 [sample, lane]\nsource r2 [sample, lane]\n").expect("pipeline");
    let recipe = parse_input_spec(
        "\
inputs:
    r1: fastq/{sample}_L{lane}_R1.fastq.gz
    r2: fastq/{sample}_L{lane}_R2.fastq.gz
require [sample, lane] where r1 count=1
require [sample, lane] where r2 count=1
",
    )
    .expect("recipe");
    let result = recipe
        .resolve(&pipeline, InputSource::Discover(&tree.0))
        .expect("scan");
    assert_eq!(result.inventory.artifacts.len(), 1);
    assert_eq!(result.gaps.len(), 1);
    assert!(result.gaps[0].error.to_string().contains("r2"));
}

#[test]
fn empty_directory_observes_a_group_and_only_require_makes_it_mandatory() {
    let tree = Tree::new("empty-sample", &[]);
    fs::create_dir_all(tree.0.join("samples/A")).expect("sample folder");
    let pipeline = parse_pipeline("source reads [sample]\n").expect("pipeline");
    let recipe = parse_input_spec(
        "\
inputs:
    reads: fastq/{sample}.fq
    contexts: samples/{sample}
require [sample] where reads count=1
",
    )
    .expect("recipe");
    let result = recipe
        .resolve(&pipeline, InputSource::Discover(&tree.0))
        .expect("scan");
    assert_eq!(result.inventory.contexts.len(), 1);
    assert_eq!(result.gaps.len(), 1);
}

#[test]
fn global_requirement_checks_singleton_lookup() {
    let tree = Tree::new("global-lut", &["monthly/2025-01.csv"]);
    let pipeline = parse_pipeline("source rows [month]\nsource lut\n").expect("pipeline");
    let recipe = parse_input_spec(
        "\
inputs:
    rows: monthly/{month}.csv
    lut: reference/lut.tsv
require [] where lut count=1
",
    )
    .expect("recipe");
    let result = recipe
        .resolve(&pipeline, InputSource::Discover(&tree.0))
        .expect("scan");
    assert_eq!(result.gaps.len(), 1);
    assert!(result.gaps[0].error.to_string().contains("lut"));
}

#[test]
fn context_target_counts_only_its_exact_dimension_domain() {
    let tree = Tree::new("context-counts", &[]);
    fs::create_dir_all(tree.0.join("samples/A/visit-1")).expect("visit");
    fs::create_dir_all(tree.0.join("samples/A/visit-2")).expect("visit");
    let pipeline = parse_pipeline("source image [sample, visit]\n").expect("pipeline");
    let recipe = parse_input_spec(
        "\
inputs:
    image: images/{sample}_{visit}.png
    contexts: samples/{sample}
    contexts: samples/{sample}/visit-{visit}
require [sample] where contexts[sample, visit] count>=2
",
    )
    .expect("recipe");
    let result = recipe
        .resolve(&pipeline, InputSource::Discover(&tree.0))
        .expect("scan");
    assert!(result.gaps.is_empty(), "{:?}", result.gaps);
    assert_eq!(result.inventory.contexts.len(), 3);
    let text = render_source_inventory(&result.inventory, &pipeline, &recipe.rules);
    assert!(text.contains("contexts:\n"), "{text}");
    assert!(!text.contains("contexts context_"), "{text}");
    let read = parse_source_inventory(&text).expect("inventory round trip");
    assert_eq!(read.contexts, result.inventory.contexts);
}
