use spit::{
    render_dag, resolve, ArtifactInstance, EntityBinding, InputBinding, InputPort, Invocation,
    OperationDef, Pipeline, ProductDef, ShapeRule,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("bold", "BOLD", &["sub", "ses", "run"]),
            ProductDef::new("t1w", "T1w", &["sub", "ses"]),
            ProductDef::new("denoised", "DenoisedBOLD", &["sub", "ses", "run"]),
            ProductDef::new("registered", "RegisteredBOLD", &["sub", "ses", "run"]),
            ProductDef::new("mean_bold", "MeanBOLD", &["sub", "ses"]),
        ],
        sources: vec![
            ArtifactInstance::new(
                "bold",
                "BOLD",
                EntityBinding::from_pairs([("sub", "01"), ("ses", "01"), ("run", "1")]),
            ),
            ArtifactInstance::new(
                "bold",
                "BOLD",
                EntityBinding::from_pairs([("sub", "01"), ("ses", "01"), ("run", "2")]),
            ),
            ArtifactInstance::new(
                "t1w",
                "T1w",
                EntityBinding::from_pairs([("sub", "01"), ("ses", "01")]),
            ),
        ],
        operations: vec![
            OperationDef::new(
                "denoise",
                vec![InputPort::one("input", "BOLD")],
                "DenoisedBOLD",
                ShapeRule::Preserve,
            ),
            OperationDef::new(
                "register",
                vec![
                    InputPort::one("moving", "DenoisedBOLD"),
                    InputPort::one("reference", "T1w"),
                ],
                "RegisteredBOLD",
                ShapeRule::Preserve,
            ),
            OperationDef::new(
                "mean",
                vec![InputPort::many("input", "RegisteredBOLD")],
                "MeanBOLD",
                ShapeRule::Aggregate,
            ),
        ],
        invocations: vec![
            Invocation::new("denoise", vec![InputBinding::product("bold")], "denoised"),
            Invocation::new(
                "register",
                vec![
                    InputBinding::product("denoised"),
                    InputBinding::product("t1w"),
                ],
                "registered",
            ),
            Invocation::new(
                "mean",
                vec![InputBinding::vary("registered", "run")],
                "mean_bold",
            ),
        ],
    };

    let dag = resolve(&pipeline)?;
    print!("{}", render_dag(&dag));
    Ok(())
}
