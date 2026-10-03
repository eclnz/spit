//! One dimension order for a pipeline: sources give it, a `dimensions`
//! line declares it where they leave a pair unordered, and every product a
//! step makes follows it.

use spit::{parse_pipeline, parse_source_inventory, resolve};

const SWEEP: &str = "\
source model [model]
source config [config]
source seed [config, seed]
operation train(model: Model, config: Config, seed: Seed) -> Weights
trained = train(model @ each(model), config, seed)
operation summarise(runs: many Weights) -> Summary
summary = summarise(trained @ vary(seed))
operation board(summaries: many Summary) -> Table
table = board(summary @ vary(model, config))
";

const RUNS: &str = "sources:
    model[model=small]
    model[model=large]
    config[config=fast]
    config[config=deep]
    seed[config=fast,seed=1]
    seed[config=deep,seed=1]
";

fn error(text: &str) -> (usize, String) {
    let error = parse_pipeline(text).unwrap_err();
    (error.line(), error.message().to_owned())
}

#[test]
fn a_pair_no_source_orders_must_be_declared() {
    let (line, message) = error(SWEEP);
    assert_eq!(line, 5);
    assert_eq!(
        message,
        "`trained` has dimensions `config` and `model`, which no source orders; \
         declare the order once with `dimensions [model, config, seed]`"
    );
}

#[test]
fn every_product_follows_the_declared_order() {
    let text = format!("dimensions [model, config, seed]\n{SWEEP}");
    let pipeline = parse_pipeline(&text).unwrap();
    let dimensions = |name: &str| {
        pipeline
            .products
            .iter()
            .find(|product| product.name == name)
            .unwrap()
            .dimensions
            .clone()
    };
    assert_eq!(dimensions("trained"), ["model", "config", "seed"]);
    assert_eq!(dimensions("summary"), ["model", "config"]);
    let dag = resolve(&pipeline, &parse_source_inventory(RUNS).unwrap()).unwrap();
    let board = dag.jobs.last().unwrap();
    let members: Vec<_> = board.inputs[0]
        .iter()
        .map(|id| dag.artifact(*id).entities.get("model").unwrap().to_owned())
        .collect();
    assert_eq!(members, ["large", "large", "small", "small"]);

    // The other order collects config-first.
    let text = format!("dimensions [config, seed, model]\n{SWEEP}");
    let pipeline = parse_pipeline(&text).unwrap();
    let dag = resolve(&pipeline, &parse_source_inventory(RUNS).unwrap()).unwrap();
    let members: Vec<_> = dag.jobs.last().unwrap().inputs[0]
        .iter()
        .map(|id| dag.artifact(*id).entities.get("config").unwrap().to_owned())
        .collect();
    assert_eq!(members, ["deep", "deep", "fast", "fast"]);
}

#[test]
fn sources_must_agree_with_each_other_and_with_the_line() {
    let (line, message) = error("source a [sub, ses]\nsource b [ses, sub]\n");
    assert_eq!(line, 2);
    assert!(
        message.contains("`b` lists `ses` before `sub`, but `a` puts `sub` first"),
        "{message}"
    );

    let (line, message) = error("dimensions [sub, ses]\nsource a [ses, sub]\n");
    assert_eq!(line, 2);
    assert!(
        message.contains(
            "`a` lists its dimensions as [ses, sub], but `dimensions` orders them [sub, ses]"
        ),
        "{message}"
    );
}

#[test]
fn the_line_names_every_dimension_once_and_no_other() {
    let (line, message) = error("dimensions [sub]\nsource a [sub, ses]\n");
    assert_eq!(line, 1);
    assert!(
        message.contains("`dimensions` leaves out `ses`, a dimension of `a`"),
        "{message}"
    );

    let (_, message) = error("dimensions [sub, run]\nsource a [sub]\n");
    assert!(
        message.contains("names `run`, which no product has"),
        "{message}"
    );

    let (_, message) = error("dimensions [sub, sub]\nsource a [sub]\n");
    assert!(message.contains("repeats `sub`"), "{message}");

    let (line, message) = error("dimensions [sub]\nsource a [sub]\ndimensions [sub]\n");
    assert_eq!(line, 3);
    assert!(message.contains("one `dimensions` line"), "{message}");

    let (_, message) = error("source a [sub]\nstage prep:\n    dimensions [sub]\n");
    assert!(message.contains("belongs at the top level"), "{message}");

    let recipe = spit::parse_input_spec("dimensions [sub]\n").unwrap_err();
    assert!(
        recipe.to_string().contains("belongs in the .spit pipeline"),
        "{recipe}"
    );
}

#[test]
fn a_written_order_is_a_check_not_an_override() {
    let text = format!(
        "dimensions [model, config, seed]\n{}",
        SWEEP.replace("summary = ", "summary : Summary [config, model] = ")
    );
    let (line, message) = error(&text);
    assert_eq!(line, 8);
    assert!(
        message.contains(
            "`summary` lists its dimensions as [config, model], but the pipeline orders them [model, config]"
        ),
        "{message}"
    );
    let text = text.replace("[config, model] =", "[model, config] =");
    assert!(parse_pipeline(&text).is_ok());
}

#[test]
fn a_product_may_be_named_dimensions() {
    let pipeline =
        parse_pipeline("source raw [id]\noperation copy(raw)\ndimensions = copy(raw)\n").unwrap();
    assert_eq!(pipeline.products[1].name, "dimensions");
}
