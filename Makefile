PREFIX ?= /usr/local
DESTDIR ?=
BUILD_DIR ?= _build

.PHONY: all bytecode test test-bytecode format check-format install clean

all:
	./ocaml/build.sh spit "$(BUILD_DIR)" native

bytecode:
	./ocaml/build.sh spit "$(BUILD_DIR)/bytecode" bytecode

test: all
	./ocaml/build.sh parser_tests "$(BUILD_DIR)/parser" native
	"$(BUILD_DIR)/parser/parser_tests" "$(CURDIR)/ocaml/examples"
	python3 ocaml/tests/compiler.py "$(abspath $(BUILD_DIR))/spit" "$(CURDIR)"
	python3 ocaml/tests/cli.py "$(abspath $(BUILD_DIR))/spit" "$(CURDIR)"

test-bytecode: bytecode
	./ocaml/build.sh parser_tests "$(BUILD_DIR)/parser-bytecode" bytecode
	"$(BUILD_DIR)/parser-bytecode/parser_tests" "$(CURDIR)/ocaml/examples"
	python3 ocaml/tests/compiler.py "$(abspath $(BUILD_DIR))/bytecode/spit" "$(CURDIR)"
	python3 ocaml/tests/cli.py "$(abspath $(BUILD_DIR))/bytecode/spit" "$(CURDIR)"

format:
	./ocaml/format.sh
	uv run --locked --only-group format ruff format runner/src runner/tests ocaml/tests ocaml/tools release

check-format:
	./ocaml/format.sh --check
	uv run --locked --only-group format ruff format --check runner/src runner/tests ocaml/tests ocaml/tools release

install: all
	install -d "$(DESTDIR)$(PREFIX)/bin"
	install -m 755 "$(BUILD_DIR)/spit" "$(DESTDIR)$(PREFIX)/bin/spit"

clean:
	rm -rf "$(BUILD_DIR)"
