#!/bin/sh
set -eu
build_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
build_target=${1:?target required: spit, parse or parser_tests}
build_directory=${2:?build directory required}
case "${3:-bytecode}" in
    bytecode) build_compiler=ocamlc; build_extension=cmo; build_unix=cma ;;
    native) build_compiler=ocamlopt; build_extension=cmx; build_unix=cmxa ;;
    *) printf '%s\n' 'build mode must be bytecode or native' >&2; exit 1 ;;
esac
case "$build_target" in
    spit|parse|parser_tests) ;;
    *) printf '%s\n' "unknown build target: $build_target" >&2; exit 1 ;;
esac
mkdir -p "$build_directory"
build_directory=$(CDPATH= cd -- "$build_directory" && pwd)
cd "$build_directory"
build_version=$(cat "$build_root/../VERSION")
case "$build_version" in
    ''|*[!0-9.]*) printf '%s\n' 'invalid VERSION' >&2; exit 1 ;;
esac
printf 'let value = "%s"\n' "$build_version" > version.ml
"$build_compiler" -c version.ml
build_objects=version
compile() {
    build_source=$1
    build_module=$(basename "$build_source" .ml)
    "$build_compiler" -I +unix -w +a-4-40-41-42-44-45-48-70 -warn-error +a -I "$build_directory" \
        -c -o "$build_directory/$build_module.$build_extension" "$build_root/$build_source.ml"
    build_objects="$build_objects $build_module"
}
for build_source in \
    src/syntax/ast src/syntax/source src/syntax/diagnostics src/syntax/span \
    src/syntax/pattern src/syntax/grammar src/syntax/scope src/syntax/layout src/syntax/template src/syntax/parser \
    src/output/json; do
    compile "$build_source"
done
case "$build_target" in
    spit)
        for build_source in \
            src/data src/compile/types src/compile/rename src/compile/loader \
            src/compile/scalar src/compile/calculation src/compile/execution \
            src/compile/expand src/compile/selection src/compile/command_check src/compile/check \
            src/plan/inventory src/plan/resolve src/bind/paths src/bind/command src/plan/recipe src/output/dag \
            bin/options bin/spit; do
            compile "$build_source"
        done
        ;;
    parse) compile src/output/syntax_json; compile bin/parse ;;
    parser_tests) compile src/output/syntax_json; compile tests/parser_tests ;;
esac
set --
for build_module in $build_objects; do
    set -- "$@" "$build_directory/$build_module.$build_extension"
done
"$build_compiler" -I +unix -I "$build_directory" -o "$build_directory/$build_target" unix.$build_unix "$@"
