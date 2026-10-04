# Plan: narrow a source placeholder to a shape (issue #61)

Status: built, except the editor extension (see below). Decisions taken by
the maintainer: shapes `digits`, `year` and `date` only; `{name:shape}`; shapes
also in `discover` patterns; `--suggest` writes them; two adjacent shapes of any
length are rejected; no precedence between a shaped and a general rule; a shape
in a pipeline or stage `path:` default or on an output is an error; `date`
checks real days, and `--suggest` shares that check. Steps 1 to 7 and 9 landed
in `a6841b0` (one commit, since `Piece` and the suggester change with the
template); the built-in shape words in `8556d62`. Step 8 is for the
`spit-vscode` repository. This file is deleted in the last commit
before the work merges, as `AGENTS.md` says.

## The problem, reproduced

Usability scenario `s1-logs` with `path log: logs/{server}/{date}.log` and these
extra files under `logs/web1` and `logs/web2`: `notes.log`, `readme.log`,
`2026-9-1.log`, `20260901.log`, `2026-09-01-final.log`, `2026_09_02.log`.
`spit inputs` reads all but the last as artifacts and says only `found 25 source
artifacts`:

```text
log[server=web1,date=notes]            log[server=web1,date=2026-9-1]
log[server=web2,date=readme]           log[server=web1,date=20260901]
log[server=web1,date=2026-09-01-final]
```

`2026_09_02.log` is already unmatched, because a value holds only ASCII letters,
digits and `-` (`is_value_character` in `src/inputs/pattern.rs`, the other half
of `encode_component`). `{date:date}` is a syntax error today (`omits dimension
`date``), because a placeholder name is read whole and a dimension is an
identifier, so no existing rule can contain `:` inside braces. A new syntax
therefore breaks no existing file.

The issue is not solved by anything on `dev`: the language reference documents the
workaround (check the count, `exclude`, or name more of the path) and the
usability round's difficulties list asks for exactly this. The issue is
described correctly. One detail it leaves out: both participants' data was
clean, so the request is about a stray file arriving later, not a wrong result
they saw.

## How matching works today

- `path_pattern` (`src/inputs/pattern.rs`) turns a template into `Piece`s:
  `Literal(String)` or `Value(dimension)`. Both `discover` rules
  (`DiscoveryPattern::new`) and source rules build the same pieces.
- `match_pattern` matches a whole path against pieces. A value is a run of
  `[A-Za-z0-9-%]`, so it never crosses `/`, `_`, `.` or `=`. When the next piece
  is a literal starting with a non-value character, `forced_end` fixes the
  value's length; otherwise every length is tried, shortest first, with failed
  positions memoised. A dimension written twice must bind the same text.
- `match_pattern` returns the encoded text per dimension; `read_binding` then
  decodes it and `readable_value` rejects text SPIT would not write itself.
- Callers: `discover` (`first_match`, `source_record`), `unmatched_files`,
  `OutputPaths` (where a pipeline's outputs lie), `--suggest`
  (`matches_its_files`, `naming.rs`), and `reach`/`missed_source` for the
  "nearest file" warning.
- A file matching two sources' rules is an error at the file
  (`source_record`: "matches the path rules of both"). There is no static
  overlap check between rules and no precedence.
- The extension is the text after the last placeholder, from its first `.`
  (`PathTemplate::extension`). A value never holds `.`, so a literal dot after a
  placeholder always ends it.
- `.spitout` keeps rules as text (`source_paths:`); `.spitdag` holds only the
  concrete paths of found artifacts, never a rule.

A shape check belongs in `match_from`, where a value's end is chosen, because
only there can a failing shape make the next candidate length, or the next
rule, be tried.

## Options

All five keep the file-level behaviour that a file failing the rule is simply
unmatched. "Cost" is per file matched.

**(a) A closed set of named shapes, `{name:shape}`.** Shapes `digits`, `year`,
`date`. Matching: after a value's end is chosen, the shape tests that slice
(`date` is 10 bytes, `year` 4, `digits` 1 or more). A fixed-length shape also
fixes the end like `forced_end`, so `{date:date}-{run}` stops trying splits.
Errors are one line each (see below). `--suggest` already knows `is_date` and
`is_year`, so it can write the shape. Cost: no allocation, a byte loop over a
slice already scanned; an unshaped rule pays one `Option` test. Extension: grammar
plus hover words. Teaching: one sentence and three names. No format change.

**(b) A mini-language of character classes with counts**, such as
`{date:d4-d2-d2}` or `{run:d+}`. Matches any shape the author can spell, but the
spelling is a new language to learn and to get wrong (`d2` or `dd`?), the
suggestions are unreadable, and every error is about the spelling, not the data.
Classes written with `[0-9]` collide with the `[...]` optional groups that
`parse_parts` already reads, and counts written `{4}` collide with the braces
placeholders use. Cost similar to (a). Rejected: more power than the scenarios
ask for, at a higher teaching cost.

**(c) Regular expressions**, `{date:/\d{4}-\d\d-\d\d/}`. Needs the `regex` crate or
a hand-written engine; `AGENTS.md` makes a new dependency earn its place with
`profiling/bench.py` numbers, and the engine would run per file. `{`, `}`, `[`,
`]` and `/` all already mean something in a path template, so a pattern needs a
second escaping layer. A pattern can match `.` or `/`, which breaks the
one-name rule, the extension rule and `forced_end`. Unbounded patterns also
make the "which of two rules" question undecidable in general. Rejected.

**(d) Filter after matching, instead of in the rule**, such as a recipe line
`keep log where date is date`, or `exclude log[date!=date]`. Needs no path
syntax and no editor grammar change, and the shape can be added without
touching the rule. But a filtered file still matches the rule, so it never
appears in the unmatched list or the nearest-file warning, two rules still
overlap on it, and `exclude` today records each removed artifact in the
`.spitout`, so a pattern would either write one `removed:` line per stray or
hide them. The rule and its restriction live on different lines, so the
rule no longer says what it reads. `exclude` also compares values as written
and has no patterns. Kept as a possible later addition for values, not for
path structure.

**(e) A shape on the dimension where the source declares it**, `source log : Log
[server, date:date]`. One place for every rule, including `{@labels}` and
`{@entities}` rules, which have no placeholder to annotate. But a dimension
name is used as a plain key in `where`, `vary(...)`, `exclude [date=..]`,
discovery rules, `.spitdag`, and every derived product inherits it, so a
shape would travel into places that cannot use it. Path layouts also differ by
dataset and are chosen in the recipe, while the dimension belongs to the
pipeline. Rejected for v1; reconsider if `{@labels}` sources need shapes.

## Recommendation: option (a)

Syntax `{name:shape}` in a source's path rule, in a recipe's `path:` or
`path source:` line, in `source_paths:`, and in a `discover ... from dirs`
pattern, where `match_pattern` is shared and a shape costs nothing extra.

| Shape | Matches | Notes |
| --- | --- | --- |
| `digits` | one or more ASCII digits | leading zeros kept; the value is the text as written |
| `year` | four digits, 1900 to 2099 | the range `--suggest` uses |
| `date` | `YYYY-MM-DD`, year 1900 to 2099, a real month and day | one shared function with `--suggest` |

No other shapes in v1; `letters` and `alnum` are the likely next ones.
Shapes test the encoded text, and none can hold `%`, `.`, `_`, `/` or a
space, so decoding and `readable_value` are unaffected.

### Edge cases

- **Adjacent placeholders.** `{date:date}-{run:digits}` matches `2026-09-01-3`
  with one split, because `date` is fixed at 10 bytes. Without shapes the same
  rule tries `date=2026` first. Two variable shapes with nothing between them,
  `{a:digits}{b:digits}`, keep today's shortest-first split, as `{a}{b}` does
  now; `--suggest` never writes one. A decision for the maintainer: reject it.
- **Partial names.** `sub-{sub:digits}_T1w` and `sub-{sub:digits}-T1w` match
  `sub-01_T1w` and `sub-01-T1w`. Today the second also reads `sub-01-02-T1w` as
  `sub=01-02`; with `digits`, which stops at `-`, it is unmatched.
- **Dots.** No shape contains `.`. The extension is still read from the first `.`
  after the last placeholder, so `{date:date}.log.gz` has extension `.log.gz`,
  and a shape never moves where it starts. `2026-09-01.log.1` stays unmatched.
- **Case.** Values compare as written. `digits`, `year`, `date` have no letters;
  a later `letters` shape accepts both cases and never folds them, as
  `pricing/S07.json` is not `s07` today.
- **A dimension written twice**, `{id:digits}/{id}`: every occurrence must
  satisfy a shape given to any of them; two different shapes on one dimension
  are an error.
- **Built-in placeholders** `{@entities}`, `{@labels}`, `{@product}`, `{@stage}`
  take no shape (error), so a source that uses `{@labels}` writes
  `sub-{sub:digits}` instead.
- **Failing the shape.** The file is unmatched, so it is counted in the existing
  note ("N files match no source rule", by extension, or named when at most
  three) and listed by `--unmatched`. In the repro above, six strays and
  nothing silently read. When the source then matches no file, `missed_source`
  names the nearest file, and `reach` must stop at a failed shape with the
  rule shown as written: "after `logs/web1/`, the file has `notes.log` where
  the rule has `{date:date}.log`".
- **Two rules.** A shaped rule and a general rule on the same folder, such as
  `{date:date}.log` and `{name}.log`, still overlap on date files and still give
  "matches the path rules of both". Shapes do not add precedence; the way out is
  shaping both (`digits` or `year`) or naming more of the path. A decision for the
  maintainer if the catch-all pattern turns out to be common.
- **Outputs.** A shape on an output's rule, or in a pipeline `path:` default
  that outputs also use, is an error. Writing a path ignores shapes, so
  accepting one there would claim a check that never runs. A recipe's `path:`
  default is only for sources and may hold shapes.
- **Records that disagree.** `locate_sources` rejects a record (`.spitout` or
  hand-written) whose value fails its rule's shape, since the file it points to is
  one discovery would no longer read.

### Messages

```text
unknown shape `dat` in `{date:dat}`; the shapes are `digits`, `year` and `date`
`{date:date}` has a shape, but shapes narrow a source's path rule only; `digest` is made by a step
`{@labels:digits}`: a built-in placeholder takes no shape; write the dimension, as `sub-{sub:digits}`
`{id:digits}` and `{id:year}`: one dimension has two shapes in the same rule
source `log[server=web1,date=notes]` has `date` that is not a `date`, which its rule `logs/{server}/{date:date}.log` reads
```

### Effect elsewhere

- **`--suggest`.** For a dimension whose values are all dates or all years it
  writes `{date:date}` or `{year:year}` in the rule it prints, and
  `matches_its_files` checks the rule with the same shapes. The suggested rule
  then also leaves stray files unmatched, which the next scan shows. `digits` is
  never suggested. Decision for the maintainer: whether to suggest at all, since
  it makes every pasted date rule longer. `is_date` is stricter after this
  change (month 1 to 12), so a column of `2024-99-99` is `dim1` again.
- **Performance** (rules in `docs/architecture.md#performance`). Checking a
  shape is a pure function over a slice, so no allocation, no new table, no
  per-file string. Rule 4 (find once): the shape is parsed with the template,
  never from text at match time. `Piece::Value(String)` becomes
  `Piece::Value(String, Option<Shape>)`, with `Shape` a `Copy` enum. Fixed-length
  shapes remove candidate splits, so shaped rules should not be slower and a
  `{date:date}-{run}` rule faster. Still benchmarked with `profiling/bench.py
  pipeline` and `dataset` as `AGENTS.md` requires, and output must stay
  byte-identical for rules without shapes.
- **`.spitdag` and `docs/spitdag.md`.** None: it holds found paths, not rules, so
  no spit-bash change.
- **VS Code extension** (`eclnz/spit-vscode`; cannot be edited from here). Needed
  in the same-named branch: `syntaxes/spit.tmLanguage.json` must scope a
  placeholder `{name:shape}` so `:` and the shape name colour as a unit, and
  `grammar.test.js` must cover `{date:date}` in a path line, a recipe `path:`,
  a `source_paths:` line and `discover`. If shape names become built-in words in
  `src/builtins.rs` (recommended), `check --json --hovers` carries their hover
  text through `words` and `word_docs` with no extension code; `extension.test.js`
  should assert one such hover. The README's keyword list gains the three
  shapes. `check --json` paths and hints show the rule text, which keeps the
  shape. Bump the extension with SPIT's version; link the two PRs, SPIT first.

## Steps and acceptance tests

Each step is one commit with its docs; tests go in a new
`tests/placeholder_shapes.rs` unless another file fits.

1. **`Shape` and the template.** A `Shape` enum with `matches(&str) -> bool` in
   `src/paths` (shared layer), used by `inputs` and the suggester. Parse
   `{name:shape}` in `PathPlaceholder::parse`; the dimension keeps an
   `Option<Shape>`; `Display` and `render` write it back, so `.spitout` and
   `check --path-rules` round-trip. Tests: parse and render each shape; unknown
   shape and built-in-with-shape errors; `{a:x}` where a dimension is missing keeps
   today's errors.
2. **Matching.** `Piece::Value` carries the shape; `match_from` tests it in the
   candidate loop and the repeated-dimension branch; `forced_end` or a
   fixed-length path uses it. Tests: with the repro files above, only the
   five date-shaped names and the real logs match `logs/{server}/{date:date}.log`;
   `{date:date}-{run:digits}` splits uniquely; `sub-{sub:digits}_T1w`;
   `{year:year}{n:digits}`; leading zeros kept; `date` rejects `2026-13-01` and
   `2026-02-30`; a second occurrence of a dimension is checked.
3. **Diagnostics.** `reach`, `missed_source` and `render` show shapes; shaped
   strays are in the unmatched note and `--unmatched`. Tests: nearest-file text for
   `notes.log`; unmatched count on the repro; two overlapping rules still say
   "matches the path rules of both".
4. **Where shapes are allowed.** Source rules, recipe `path:` and `path source:`,
   `source_paths:`, `discover` patterns. Errors for outputs and shared defaults;
   `locate_sources` rejects a mismatching record. Tests for each place and each
   error.
5. **`--suggest`.** Write `{date:date}`/`{year:year}`, share `is_date`/`is_year`
   from step 1, check suggested rules with shapes. Tests: the suggestion for
   `s1-logs` data pastes back and matches the same files; stored outputs under
   `tests/fixtures/outputs/` re-saved only if they change, with the diff read.
6. **Docs.** Replace the workaround paragraph in `docs/language-reference.md`
   (Paths, "Path rules also find sources") with the shapes, with a verified
   example; update the `--suggest` paragraph in `README.md`; add the three
   shape names to `src/builtins.rs`; note in `docs/architecture.md` that
   shapes are checked inside `match_from`.
7. **Benchmark.** `profiling/bench.py pipeline` and `dataset` with `--old` set to
   the commit before step 2; the result goes in that commit's message.
8. **Editor and runner.** Extension branch of the same name as above. No
   spit-bash change.
9. **Usability.** Use `{date:date}` in the `s1-logs` key and run
   `usability/harness/rebuild_keys.sh`; expect `ok` for every key.

## Decisions for the maintainer (all taken as in the status above)

1. Shape set for v1: `digits`, `year`, `date` only, or also `letters`/`alnum`?
2. Syntax `{name:shape}`, or a different separator?
3. Allow shapes in `discover ... from dirs` patterns (recommended), or sources only?
4. `--suggest`: write shapes for dates and years, or only report them?
5. Reject two adjacent variable-length shapes, `{a:digits}{b:digits}`?
6. A shaped rule overlapping a general one: keep the per-file error, or add a
   rule that the shaped rule wins?
7. Is a shape in a pipeline-level `path:` default an error (recommended) or
   ignored for outputs?
8. Should `date` check real calendar days, which makes `--suggest` stricter too?

## Step status

1. Shape and template: done, `a6841b0`.
2. Matching: done, `a6841b0`.
3. Diagnostics: done, `a6841b0`.
4. Where shapes are allowed: done, `a6841b0`.
5. `--suggest`: done, `a6841b0`.
6. Docs: done, `a6841b0`; shape words in `src/builtins.rs`, `8556d62`.
7. Benchmark: passed, in the message of `a6841b0`.
8. Editor and runner: not done here; spit-bash needs nothing.
9. Usability: `rebuild_keys.sh` is `ok` for every key; the `s1-logs` key still
   writes `{date}`, and is left as it is, since the scenario is a record.
