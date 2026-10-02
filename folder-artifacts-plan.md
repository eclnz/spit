# Folder artifacts plan

Let a source or an output be a folder rather than a file, for tools that read or write a folder of files: a DICOM series, a FreeSurfer subject, a Zarr store.

Work is on branch `claude/folder-artifacts` in spit, spit-bash and spit-vscode.

## Decisions

- **Syntax.** A trailing `/` in the extension's place marks a folder: `source dicom : Dicom / [sub]`, `-> (subject: FsSubject /)`, `-> Store .zarr/`. No new keyword.
- **`ext:` never applies to a folder.** A folder's extension is only one it declares.
- **No `beside` or `sidecars` folders.** A `beside` output cannot be a folder or follow one; a `sidecars` member cannot be a folder.
- **What may sit inside a folder artifact.** A source may sit inside a source folder, since nothing writes to either. An output inside any folder artifact, or a source inside an output folder, is an error: two jobs would write to one place, or the runner's clearing of the folder would delete a source.
- **`.stem` of a folder output without an extension** is its whole name, so `recon-all -sd {subject.dir} -s {subject.stem}` works.
- **`.spitdag` version 5.** Every artifact gets `"kind": "file"` or `"folder"`. Every fingerprint changes once.
- **The runner** creates a folder output's parent, deletes a folder output left by an earlier run before the command runs, and requires the folder to exist after. A folder's stamp summarises the files under it, so editing a file inside counts as a change.

## Steps

1. [ ] Model and parser: `folder` on `ProductDef` and `OutputPort`, `PipelineIndex::is_folder`, the `/` syntax, the restrictions above.
2. [ ] Path checks: outputs inside folders, `.stem` of an extensionless folder.
3. [ ] Discovery: split `src/inputs/discover.rs`; folder sources match directories; files inside a found folder are not unmatched; required folders are checked as folders.
4. [ ] Binding in `dag`: a folder source must be a folder.
5. [ ] `.spitdag` version 5 with `kind`; `dag --paths` marks folders.
6. [ ] Editor output: hovers show a folder's `/`.
7. [ ] Docs: language reference, README, `docs/spitdag.md`, architecture.
8. [ ] spit-bash: version 5, folder stamps, clearing and checking folder outputs.
9. [ ] spit-vscode: grammar, README, tests.
