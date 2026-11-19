//! Declarations shared by both forms: products, operations and their ports,
//! input bindings, commands, path rules and imports. A recipe's `require`
//! and conditional `exclude` rules are in `rules.rs`.

use std::collections::BTreeMap;

use crate::model::{ArtifactSelection, Beside, InputBinding, Invocation, Port, ProductDef};
use crate::template::{PathGlob, PathTemplate};
use crate::types::{parse_type_expr, TypeExpr, TypeParseError};

use super::check::{split_clauses, Clause};
use super::error::DeclarationError;
use super::lexical::{
    call_parts, comma_items, identifier, product_name, qualified_identifier, split_ending,
};
use super::operation::expected_clause;
use super::review::{is_attachment, only_attachments, source_attachments, Attached};
use super::scan::{self, find_outside};
use super::source_map::tail_place;
use super::{ClauseSite, ParseError, ParseErrorKind, PathRule, StatementKind};

pub(super) fn single_colon(line: &str) -> Option<usize> {
    line.char_indices().find_map(|(index, character)| {
        (character == ':' && !line[..index].ends_with(':') && !line[index + 1..].starts_with(':'))
            .then_some(index)
    })
}

/// Parse a path rule from `rest`, the text after `path`: `: template` for
/// the default rule, which inside `stage` covers only that stage's
/// products, or `product: template` for one product's. A product's rules
/// may instead be on the lines beneath it, one to a line.
pub(super) fn parse_path(
    stage: Option<String>,
    original: &str,
    rest: &str,
    number: usize,
) -> Result<PathRule, ParseError> {
    let (product, template) = if let Some(template) = rest.strip_prefix(':') {
        (None, template)
    } else {
        let declaration = rest.trim_start();
        let delimiter = single_colon(declaration).ok_or_else(|| {
            ParseError::of(number, DeclarationError::ExpectedColonAfterPathProduct)
        })?;
        let (product, template) = declaration.split_at(delimiter);
        (
            Some(qualified_identifier(product.trim(), number, "path product")?.to_owned()),
            &template[1..],
        )
    };
    let (after, beneath) = template.split_once('\n').unwrap_or((template, ""));
    let mut rules = Vec::new();
    for (line, text) in std::iter::once(after)
        .chain(beneath.lines())
        .enumerate()
        .map(|(offset, text)| (number + offset, unquoted(text.trim())))
        .filter(|(_, text)| !text.is_empty())
    {
        rules.push(PathTemplate::parse(text).map_err(|error| {
            ParseError::of(line, ParseErrorKind::PathTemplate(error.error)).at_token(text)
        })?);
    }
    if rules.is_empty() {
        return Err(ParseError::of(number, DeclarationError::EmptyPathTemplate));
    }
    if rules.len() > 1 && product.is_none() {
        return Err(ParseError::of(
            number,
            DeclarationError::DefaultPathHasSeveralRules,
        ));
    }
    if rules.len() > 1 && !unquoted(after.trim()).is_empty() {
        return Err(ParseError::of(
            number,
            DeclarationError::PathRulesAfterAndBeneath,
        ));
    }
    let first = original.lines().next().unwrap_or(original);
    let shown = match unquoted(after.trim()) {
        "" => product.as_deref().unwrap_or(first),
        written => written,
    };
    let place = tail_place(first, number, shown);
    Ok(PathRule {
        stage: stage.filter(|_| product.is_none()),
        product,
        template: PathTemplate::several(rules),
        place,
    })
}

/// `template` without the quotes around it, if it has a pair.
fn unquoted(template: &str) -> &str {
    template
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            template
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(template)
        .trim()
}

#[derive(Debug)]
pub(crate) struct UseSpec {
    pub(crate) names: Option<Vec<String>>,
    pub(crate) path: String,
    pub(crate) alias: Option<String>,
}

pub(crate) fn parse_use(line: &str, number: usize) -> Result<UseSpec, ParseError> {
    let rest = line
        .strip_prefix("use ")
        .ok_or_else(|| ParseError::of(number, DeclarationError::ExpectedUseForm))?;
    let (names, path_and_alias) = if rest.starts_with('"') || rest.starts_with('\'') {
        (None, rest)
    } else if let Some((names, path)) = rest.split_once(" from ") {
        let names = comma_items(names, number)?;
        if names.is_empty() {
            return Err(ParseError::of(number, DeclarationError::ExpectedUseForm));
        }
        for name in &names {
            qualified_identifier(name, number, "import name")?;
        }
        (Some(names.into_iter().map(str::to_owned).collect()), path)
    } else {
        (None, rest)
    };
    let (path, alias) = parse_use_path(path_and_alias, number)?;
    if path.is_empty() {
        return Err(ParseError::of(number, DeclarationError::ImportWithoutFile));
    }
    Ok(UseSpec { names, path, alias })
}

fn parse_use_path(text: &str, number: usize) -> Result<(String, Option<String>), ParseError> {
    let text = text.trim();
    if let Some(quote @ ('"' | '\'')) = text.chars().next() {
        let closing = text[1..]
            .find(quote)
            .map(|index| index + 1)
            .ok_or_else(|| {
                ParseError::of(number, DeclarationError::UnterminatedQuotedImportPath)
            })?;
        let path = &text[1..closing];
        let tail = text[closing + 1..].trim();
        let alias = if tail.is_empty() {
            None
        } else {
            let value = tail.strip_prefix("as ").ok_or_else(|| {
                ParseError::of(number, DeclarationError::ExpectedAliasAfterImportPath)
            })?;
            Some(identifier(value.trim(), number, "import alias")?.to_owned())
        };
        Ok((path.to_owned(), alias))
    } else if let Some((path, alias)) = text.rsplit_once(" as ") {
        Ok((
            path.trim().to_owned(),
            Some(identifier(alias.trim(), number, "import alias")?.to_owned()),
        ))
    } else {
        Ok((text.to_owned(), None))
    }
}

/// Turn a [`TypeParseError`] into a [`ParseError`] pointing at the specific
/// token within `ty` that the type parser rejected, rather than all of `ty`.
pub(super) fn type_error(number: usize, ty: &str, error: TypeParseError) -> ParseError {
    // Fall back to the whole type if a span is ever not a valid range in `ty`.
    let token = ty.get(error.span.clone()).unwrap_or(ty);
    ParseError::of(number, ParseErrorKind::Type(error)).at_token(token)
}

/// The text after `type`, as in `type Frame<Kind, Space>`: a name and the
/// names of its parameters, which are capitalized and differ.
pub(super) fn parse_type_declaration(
    text: &str,
    number: usize,
) -> Result<StatementKind, ParseError> {
    let text = text.trim();
    let declared = parse_type_expr(text, false).map_err(|error| type_error(number, text, error))?;
    let (constructor, args) = match declared {
        TypeExpr::Applied { constructor, args } => (constructor, args),
        // A type without parameters is used as it is, so a line declaring one
        // is the mistake, not a missing list of parameters.
        TypeExpr::Named(name) => {
            return Err(ParseError::of(
                number,
                DeclarationError::TypeNeedsNoDeclaration {
                    name: name.to_string(),
                },
            )
            .at_token(text));
        }
        TypeExpr::Variable(_) | TypeExpr::Unknown => {
            return Err(
                ParseError::of(number, DeclarationError::ExpectedTypeDeclaration).at_token(text),
            );
        }
    };
    let mut params: Vec<String> = Vec::new();
    for arg in args {
        let TypeExpr::Named(param) = arg else {
            return Err(
                ParseError::of(number, DeclarationError::ExpectedTypeDeclaration).at_token(text),
            );
        };
        if params.contains(&param) {
            return Err(ParseError::of(
                number,
                DeclarationError::TypeHasParameterTwice {
                    constructor: constructor.to_string(),
                    param: param.to_string(),
                },
            )
            .at_token(text));
        }
        params.push(param);
    }
    Ok(StatementKind::TypeDecl(constructor, params))
}

/// The list after `dimensions`, as in `dimensions [model, config, seed]`.
pub(super) fn parse_dimension_order(text: &str, number: usize) -> Result<Vec<String>, ParseError> {
    let text = text.trim();
    let list = text
        .strip_prefix('[')
        .and_then(|list| list.strip_suffix(']'))
        .ok_or_else(|| {
            ParseError::of(number, DeclarationError::ExpectedDimensionsList).at_token(text)
        })?;
    let mut order: Vec<String> = Vec::new();
    for dimension in comma_items(list, number)? {
        let dimension = identifier(dimension, number, "dimension")?;
        if order.iter().any(|known| known == dimension) {
            return Err(ParseError::of(
                number,
                DeclarationError::DimensionsRepeats {
                    dimension: dimension.to_string(),
                },
            )
            .at_token(dimension));
        }
        order.push(dimension.to_owned());
    }
    if order.is_empty() {
        return Err(ParseError::of(number, DeclarationError::DimensionsEmpty).at_token(text));
    }
    Ok(order)
}

pub(super) fn parse_product(line: &str, number: usize) -> Result<ProductDef, ParseError> {
    let (line, clauses) = split_clauses(line, ClauseSite::AfterSource, number)?;
    let attached = source_attachments(&clauses, number)?;
    // `here` ends the declaration, before any `@check` or `@review`.
    let (line, here) = match line.trim_end().strip_suffix("here") {
        Some(rest) if rest.ends_with(char::is_whitespace) => {
            (rest.trim_end(), Some(&line.trim_end()[rest.len()..]))
        }
        _ => (line, None),
    };
    let (line, beside) = match line.rsplit_once(" beside ") {
        Some((item, sibling)) => {
            if item.trim_end().ends_with(']') {
                return Err(ParseError::of(
                    number,
                    DeclarationError::BesideSourceHasDimensions,
                ));
            }
            let (sibling, name) = super::operation::naming_suffix(sibling.trim(), number)?;
            let (item, suffix, append) =
                super::operation::beside_suffix(item.trim(), name, sibling, number, "source")?;
            let sibling = identifier(sibling, number, "source beside name")?;
            (item, Some((sibling, suffix, name, append)))
        }
        None => (line, None),
    };
    let (declaration, dimensions) = match line.split_once('[') {
        Some((declaration, dimensions)) => (declaration, Some(dimensions)),
        None => (line, None),
    };
    // `name : Type .ext`: the extension follows the type, as on an output,
    // and a `/` after it makes the source a folder.
    let (named, extension, folder) = split_ending(declaration, number)?;
    let (name, artifact_type) = if let Some((name, ty)) = named.split_once(':') {
        let ty = ty.trim();
        let ty = parse_type_expr(ty, false).map_err(|error| type_error(number, ty, error))?;
        (name.trim(), ty)
    } else {
        (named.trim(), TypeExpr::Unknown)
    };
    let name = product_name(name, number, "product name").map_err(|error| {
        if dimensions.is_none() && name.split_whitespace().count() > 1 {
            ParseError::of(number, DeclarationError::ExpectedSourceForm).at_token(name)
        } else {
            error
        }
    })?;
    let dimensions = if let Some(dimensions) = dimensions {
        // From the `[` that is never closed to the end of the declaration.
        let bracketed = &line[declaration.len()..];
        let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
            ParseError::of(number, DeclarationError::UnclosedDimensions).at_token(bracketed)
        })?;
        let items = comma_items(dimensions, number)?;
        if items.is_empty() {
            return Err(ParseError::of(
                number,
                DeclarationError::EmptyBrackets {
                    name: name.to_string(),
                },
            )
            .at_token(bracketed));
        }
        items
    } else {
        Vec::new()
    };
    for dimension in &dimensions {
        identifier(dimension, number, "dimension")?;
    }
    let mut product = ProductDef::new(name, artifact_type, &dimensions);
    product.extension = extension.map(str::to_owned);
    product.folder = folder;
    product.checks = attached.checks;
    product.exempt_checks = attached.exempt;
    product.reviews = attached.reviews;
    if let Some((sibling, suffix, name, append)) = beside {
        if product.folder || !product.dimensions.is_empty() || product.extension.is_some() {
            return Err(ParseError::of(
                number,
                DeclarationError::BesideSourceHasOtherParts,
            ));
        }
        let beside = Beside {
            sibling: sibling.to_owned(),
            suffix,
            name: name.to_owned(),
            append,
        };
        product.extension = beside.extension().map(str::to_owned);
        product.beside = Some(beside);
    }
    if let Some(here) = here {
        if product.folder || !product.dimensions.is_empty() || product.beside.is_some() {
            return Err(
                ParseError::of(number, DeclarationError::HereSourceHasExtras).at_token(here),
            );
        }
        product.here = Some(String::new());
    }
    Ok(product)
}

pub(super) fn parse_invocation_parts(
    outputs: Vec<String>,
    call: &str,
    number: usize,
) -> Result<Invocation, ParseError> {
    let (operation, args) = call_parts(call.trim(), number)?;
    let mut invocation = Invocation::with_outputs(operation, Vec::new(), outputs);
    for (port, arg) in comma_items(args, number)?.into_iter().enumerate() {
        let (binding, attached) = parse_binding(arg, number)?;
        invocation.inputs.push(binding);
        invocation.checks.extend(
            attached
                .checks
                .into_iter()
                .map(|check| (Port::Input(port), check)),
        );
        invocation.reviews.extend(
            attached
                .reviews
                .into_iter()
                .map(|review| (Port::Input(port), review)),
        );
    }
    Ok(invocation)
}

/// Parse a product with selectors and gates on this use of it.
fn parse_binding(arg: &str, number: usize) -> Result<(InputBinding, Attached), ParseError> {
    // The product is read before the clauses, so a bad name is the error
    // when both are bad.
    let product = find_outside(arg, scan::ALL, '@').map_or(arg, |at| &arg[..at]);
    let mut binding = InputBinding::product(qualified_identifier(
        product.trim(),
        number,
        "input product",
    )?);
    let (_, selectors) = split_clauses(arg, ClauseSite::OnInput, number)?;
    let mut attached = Attached::default();
    for clause in selectors {
        if is_attachment(&clause) {
            let given = only_attachments(std::slice::from_ref(&clause), "a call's input", number)?;
            attached.checks.extend(given.checks);
            attached.reviews.extend(given.reviews);
            continue;
        }
        let Clause {
            keyword,
            argument: arguments,
            text: selector,
        } = clause;
        let items = comma_items(arguments, number)?;
        if items.is_empty() {
            return Err(ParseError::of(
                number,
                DeclarationError::SelectorNeedsDimension {
                    keyword: keyword.to_string(),
                },
            )
            .at_token(selector));
        }
        let duplicate = || {
            ParseError::of(
                number,
                ParseErrorKind::DuplicateClause {
                    keyword: keyword.to_string(),
                },
            )
            .at_token(keyword)
        };
        match keyword {
            "vary" => {
                if !binding.vary.is_empty() {
                    return Err(ParseError::of(number, DeclarationError::VaryInOneClause)
                        .at_token(selector));
                }
                for dimension in items {
                    let dimension = identifier(dimension, number, "vary dimension")?;
                    if binding.vary.iter().any(|name| name == dimension) {
                        return Err(ParseError::of(
                            number,
                            DeclarationError::VaryRepeats {
                                dimension: dimension.to_string(),
                            },
                        )
                        .at_token(selector));
                    }
                    binding.vary.push(dimension.to_owned());
                }
            }
            "where" => {
                if !binding.pinned.is_empty() {
                    return Err(duplicate());
                }
                binding.pinned = parse_pins(&items, number)?;
            }
            "first" | "last" => {
                let [dimension] = items.as_slice() else {
                    return Err(ParseError::of(
                        number,
                        DeclarationError::SelectionNeedsOneDimension {
                            keyword: keyword.into(),
                        },
                    )
                    .at_token(selector));
                };
                if let Some(previous) = &binding.selection {
                    return Err(ParseError::of(
                        number,
                        DeclarationError::SelectionRepeated {
                            previous: previous.keyword().into(),
                            keyword: keyword.into(),
                        },
                    )
                    .at_token(selector));
                }
                let dimension = identifier(dimension, number, "selection dimension")?.to_owned();
                binding.selection = Some(if keyword == "first" {
                    ArtifactSelection::First(dimension)
                } else {
                    ArtifactSelection::Last(dimension)
                });
            }
            "same" => {
                let dimensions = items
                    .into_iter()
                    .map(|item| identifier(item, number, "same dimension"))
                    .collect::<Result<Vec<_>, _>>()?;
                if binding.same.replace(owned(&dimensions)).is_some() {
                    return Err(duplicate());
                }
            }
            "each" => {
                if !binding.each.is_empty() {
                    return Err(duplicate());
                }
                binding.each = parse_each(&items, number)?;
            }
            _ => return Err(expected_clause(ClauseSite::OnInput, number).at_token(keyword)),
        }
    }
    Ok((binding, attached))
}

/// The `dimension=value` pins of `@where(...)`, each dimension once.
fn parse_pins(items: &[&str], number: usize) -> Result<BTreeMap<String, String>, ParseError> {
    let mut pinned = BTreeMap::new();
    for &item in items {
        let (dimension, value) = item.split_once('=').ok_or_else(|| {
            ParseError::of(number, DeclarationError::ExpectedDimensionValueWhere).at_token(item)
        })?;
        let dimension = identifier(dimension.trim(), number, "where dimension")?;
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(
                ParseError::of(number, DeclarationError::WhereValueNotOneToken).at_token(item),
            );
        }
        if pinned
            .insert(dimension.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(ParseError::of(
                number,
                DeclarationError::WherePinsTwice {
                    dimension: dimension.to_string(),
                },
            )
            .at_token(item));
        }
    }
    Ok(pinned)
}

/// The dimensions of `@each(...)`, each once.
fn parse_each(items: &[&str], number: usize) -> Result<Vec<String>, ParseError> {
    let mut each: Vec<String> = Vec::new();
    for &item in items {
        let dimension = identifier(item, number, "each dimension")?;
        if each.iter().any(|named| named == dimension) {
            return Err(ParseError::of(
                number,
                DeclarationError::EachRepeats {
                    dimension: dimension.to_string(),
                },
            )
            .at_token(item));
        }
        each.push(dimension.to_owned());
    }
    Ok(each)
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// What an `exclude` line names: a source, the values an artifact's
/// identity must include, or both, the values in the order written; or a
/// path under the root, with no source or value.
#[derive(Clone, Debug)]
pub(crate) struct ExcludeRule {
    pub(crate) product: Option<String>,
    pub(crate) values: Vec<(String, String)>,
    pub(crate) path: Option<PathGlob>,
}

/// Parse the text after `prefer`: `path` and the paths to keep, read to the
/// end of the line.
pub(crate) fn parse_prefer(text: &str, number: usize) -> Result<PathGlob, ParseError> {
    let path = text
        .trim()
        .strip_prefix("path ")
        .ok_or_else(|| ParseError::of(number, DeclarationError::PreferNeedsPath))?;
    parse_found_path(path, number)
}

/// The path of `exclude path` or `prefer path`: text and `*` under the root.
fn parse_found_path(path: &str, number: usize) -> Result<PathGlob, ParseError> {
    PathGlob::parse(path).map_err(|detail| {
        ParseError::of(number, DeclarationError::FoundPathInvalid { detail }).at_token(path.trim())
    })
}

/// Parse the text after `exclude`: a source with or without
/// `[dimension=value,...]`, `[dimension=value,...]` alone, or `path` and a
/// path under the root, read to the end of the line.
pub(crate) fn parse_exclude(text: &str, number: usize) -> Result<ExcludeRule, ParseError> {
    let text = text.trim();
    if text == "from" || text.starts_with("from ") {
        return Err(ParseError::of(number, DeclarationError::ExcludeFromReplaced).at_token(text));
    }
    if let Some(path) = text.strip_prefix("path ") {
        return Ok(ExcludeRule {
            product: None,
            values: Vec::new(),
            path: Some(parse_found_path(path, number)?),
        });
    }
    let (product, bindings) = match text.split_once('[') {
        Some((product, bindings)) => (product.trim(), Some(bindings)),
        None => (text, None),
    };
    let product = if product.is_empty() {
        None
    } else {
        Some(qualified_identifier(product, number, "source product")?.to_owned())
    };
    let values = match bindings {
        Some(bindings) => {
            // Checked as a record's values are, then kept in written order.
            super::inventory::parse_bindings(bindings, number)?;
            let inner = bindings.trim_end().trim_end_matches(']');
            comma_items(inner, number)?
                .into_iter()
                .filter_map(|item| item.split_once('='))
                .map(|(dimension, value)| (dimension.trim().to_owned(), value.trim().to_owned()))
                .collect()
        }
        None => Vec::new(),
    };
    if product.is_none() && values.is_empty() {
        return Err(ParseError::of(
            number,
            DeclarationError::ExcludeNamesNothing,
        ));
    }
    Ok(ExcludeRule {
        product,
        values,
        path: None,
    })
}
