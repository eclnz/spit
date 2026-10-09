# Custom entity path formatting (#125)

## Design

Pipeline-wide `entities: {key}_{value} separated "-"` controls key/value and assignment separators and the dimensionless spelling. The template contains {key} then {value}, once each. An optional `empty "all"` suffix changes the dimensionless spelling; the default is global. `entities sub: subject` aliases a dimension label. No scoped overrides, ordering overrides, dimension omission or named profiles: these are not needed for one shared definition across product shapes. `{@labels}` remains fixed. Keys/values retain percent encoding; separators must make discovery unambiguous. A custom empty spelling of "" can be omitted through an optional group. Imported source templates preserve an explicitly declared library format; an unconfigured imported template inherits the caller format. Caller-generated outputs use the caller format. Fresh inventories save expanded custom source rules.

Resolve custom entity placeholders into literal labels and dimension placeholders once per product; retain the existing borrowed template and artifact binding path when unconfigured. All existing default output bytes stay unchanged.

## Work

- [ ] Add format data, declarations, located validation and recovery; preserve products named entities.
- [ ] Share expansion across hints, binding, discovery and inventory serialization; cover imports and optional groups.
- [ ] Add CLI and regression tests and update an existing worked example and guides.
- [ ] Update spit-vscode on the same branch, with grammar and hint tests and linked PRs.
- [ ] Build old dev in a worktree; run pipeline/dataset byte and timing regression checks, full required checks, Messie and docs.
- [ ] Commit and create linked draft PRs, record completion and delete this plan in its own final commit.

## Validation before the implementation commit

All required release checks, answer keys, Messie and the strict docs build pass. Seven integration tests cover mixed shapes, aliases, unchanged identities and labels, optional groups, dimensionless spelling, discovery, saved inventories, imports and located failures. The editor's 53 tests pass with the feature binary, including grammar and path hints. The editor work builds on the existing multiline PR eclnz/spit-vscode#30, which carries the current diagnostic fixes; its old dev branch trails those fixes.

The old build is dev at 2b6569c. Ninety-one existing example check/input/DAG commands have identical exit statuses, stdout and stderr; the intentionally customized selectors example is excluded. Required pipeline and dataset timing checks pass with no regression. The final pipeline check is 14.3 -> 14.3 ms, JSON 14.4 -> 14.4 ms, hovers 32.0 -> 31.8 ms. No stored output fixtures changed.
