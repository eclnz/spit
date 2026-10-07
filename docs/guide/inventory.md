# Input inventory

Save a `.spitout` when you want to inspect settled inputs, pass them between tools, or plan without rescanning. A generated inventory records what the dataset policy left in, rather than asking the resolver to apply the policy again.

## Use or write a `.spitout`

`spit inputs dataset.spitin -o dataset.spitout` writes the settled source identities and source paths. A hand-written `.spitout` works too:

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
```

`spit dag pipeline.spit dataset.spitout` uses those records. A `.spitout` may also hold a `root`, `source_paths:`, `contexts`, and `removed:` records. A printed or hand-written inventory without `root` does not make `dag` check that source files exist. The [inputs reference](../manual/inventory.md#inputs) describes the complete format.

Rescan after changes to the dataset or recipe. When saving with `-o`, SPIT writes the dataset root relative to the inventory’s location. Printing to stdout leaves the root out because its destination is unknown. Add a root before using a copied inventory to verify files on disk; a rootless inventory can still be used for logical planning. See the [inventory specification](../manual/inventory.md).

Next: [Runnable DAG](dag.md).
