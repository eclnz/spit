# The editor

The [spit-vscode](https://github.com/eclnz/spit-vscode) extension shows what SPIT knows about a pipeline as you write it: errors, inferred types, each product's producer and path, and the files a step will write. It runs the same `spit check` that you run on the command line, so the editor and the terminal always agree.

## What it shows

**Colour.** It highlights `.spit` pipelines, `.spitin` recipes, `.spitout` inventories and `.spitenv` tools files. Keywords, dimension lists, `{placeholders}`, extensions, checks and reviews each have their own colour, and SPIT's own placeholders, such as `{@product}` and `{@output}`, are coloured apart from the ones a pipeline names, such as `{sub}` or `{series}`.

**Errors as you type.** After a short pause, it checks the text in the editor, saved or not, and underlines the problem: a misspelt input, an unmatched brace, a type that does not fit. Problems also appear in the Problems panel. An error inside a library that a pipeline imports is marked in the library, at its own line, with the `use` line that reads it as related information. An error in a step that a composite operation makes is marked at the call, and lists the step in the body it came from.

**Hovers.** Hover over one of SPIT's words, such as `source`, `@vary(...)`, `beside` or `{@entities}`, to see an example, a summary and a link to the [language reference](../language-reference.md). Hover over a product or an operation in a pipeline to see what the compiler knows:

- an operation's signature, and its command and checks, or its body if it is carried out by steps;
- at a call, the concrete products and types, and the type variables' values;
- for a product, its inferred type and dimensions, the step that makes it, the steps that read it, and its path rule.

For example, with `source scan : Frame<Native> [sample]` and `operation clean(frame: Frame<S>) -> CleanFrame<S>`, hovering over the call `cleaned = clean(scan)` shows `S = Native`, and hovering over `cleaned` shows `CleanFrame<Native> [sample]`. A product that inference could not type says so.

**Path hints.** When a pipeline checks clean, each step shows, at the end of its line, the path rule its output will have, such as `→ {region}/{site}/cleaned.csv`, with the dimensions left as placeholders because no dataset has been read. Optional `[...]` groups are kept or dropped for the product, and `{@entities}` is written out. These are VS Code inlay hints, so `editor.inlayHints.enabled` turns them off.

## What it does not do

- It checks one file at a time and reads no data. Settling a dataset is `spit inputs`, and the editor does not scan folders. A recipe is checked against the pipeline it names, so a rule that names an unknown source is marked, and a `root` that is not a folder is a warning.
- A `.spitout` is checked for the syntax of its records only.
- Recipes and inventories are highlighted and checked, but have no pipeline hovers.

Saving a pipeline rechecks the open recipes and the pipelines that import it.

## Setup

The extension is run from a checkout, in an Extension Development Host, rather than installed from a marketplace:

1. Build SPIT with `cargo build`, and put `spit` on your `PATH` or leave the build beside the extension.
2. Open the spit-vscode folder in VS Code, and choose **Run > Run Without Debugging**.
3. In the new window, open a pipeline. Hover over a call to see its types, and edit the file to see errors.

The extension uses `../spit/target/debug/spit` if it exists, then `spit` on `PATH`. The one setting, `spit.executablePath`, names another build, and only your user settings can change it. It stays off in folders VS Code does not trust.

The extension and SPIT share a version number. It checks `spit --version` before checking a file and needs a SPIT at least as new as itself, and the status bar shows which SPIT checked the open file. When SPIT changes what `check --json` or `--hovers` returns, rebuild SPIT and use the extension from the same release.

## How it works

For each file, the extension runs:

<!-- spit-doc: fragment -->

```sh
spit check pipeline.spit --json --stdin --hovers
```

`--json` prints diagnostics as data, and exits with status 1 when any is an error, which the extension expects. `--stdin` reads the file's text from the editor, so unsaved changes are checked, while the file's path still resolves imports. `--hovers` adds the hover text and path hints. Any editor can use the same interface.
