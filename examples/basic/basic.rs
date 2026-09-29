use spit::{
    render_dag, resolve, EntityBinding, InputBinding, InputPort, Invocation, OperationDef,
    Pipeline, ProductDef, ShapeRule, SourceInventory, SourceRecord, TypeExpr,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("bold", TypeExpr::named("BOLD"), ["sub", "ses", "run"]),
            ProductDef::new("t1w", TypeExpr::named("T1w"), ["sub", "ses"]),
            ProductDef::new(
                "denoised",
                TypeExpr::named("DenoisedBOLD"),
                ["sub", "ses", "run"],
            ),
            ProductDef::new(
                "registered",
                TypeExpr::named("RegisteredBOLD"),
                ["sub", "ses", "run"],
            ),
            ProductDef::new("mean_bold", TypeExpr::named("MeanBOLD"), ["sub", "ses"]),
        ],
        operations: vec![
            OperationDef::new(
                "denoise",
                vec![InputPort::one("input", TypeExpr::named("BOLD"))],
                TypeExpr::named("DenoisedBOLD"),
                ShapeRule::Preserve,
            ),
            OperationDef::new(
                "register",
                vec![
                    InputPort::one("moving", TypeExpr::named("DenoisedBOLD")),
                    InputPort::one("reference", TypeExpr::named("T1w")),
                ],
                TypeExpr::named("RegisteredBOLD"),
                ShapeRule::Preserve,
            ),
            OperationDef::new(
                "mean",
                vec![InputPort::many("input", TypeExpr::named("RegisteredBOLD"))],
                TypeExpr::named("MeanBOLD"),
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
        ..Pipeline::default()
    };

    let inventory = SourceInventory {
        artifacts: vec![
            SourceRecord::new(
                "bold",
                EntityBinding::from_pairs([("sub", "01"), ("ses", "01"), ("run", "1")]),
            ),
            SourceRecord::new(
                "bold",
                EntityBinding::from_pairs([("sub", "01"), ("ses", "01"), ("run", "2")]),
            ),
            SourceRecord::new(
                "t1w",
                EntityBinding::from_pairs([("sub", "01"), ("ses", "01")]),
            ),
        ],
        contexts: Vec::new(),
        ..SourceInventory::default()
    };
    let dag = resolve(&pipeline, &inventory)?;
    print!("{}", render_dag(&dag));
    Ok(())
}
