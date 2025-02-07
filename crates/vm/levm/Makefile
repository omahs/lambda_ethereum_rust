.PHONY: all test clippy fmt usage lint eth-tests

all: test clippy fmt ## 🚀 Runs all tests, linter and formatter

help: ## 📚 Show help for each of the Makefile recipes
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

test: ## 🧪 Runs all tests except Ethereum tests
	cargo test -p ethereum_rust-levm

lint: ## 🧹 Linter check
	cargo clippy --all-targets --all-features --workspace -- -D warnings

fmt: ## 📄 Runs rustfmt
	cargo fmt --all

###### Ethereum tests ######

ETHTEST_SHASUM := ".ethtest_shasum"
ETHTEST_VERSION := $(shell cat .ethtest_version)
ETHTEST_TAR := "ethtests-${ETHTEST_VERSION}.tar.gz"

${ETHTEST_TAR}: .ethtest_version
	curl -Lo ${ETHTEST_TAR} https://github.com/ethereum/tests/archive/refs/tags/${ETHTEST_VERSION}.tar.gz

ethtests: ${ETHTEST_TAR}  # 📥 Downloads Ethereum Foundation tests.
	mkdir -p "$@"
	tar -xzmf "$<" --strip-components=1 -C "$@"
	@cat ${ETHTEST_SHASUM}
	sha256sum -c ${ETHTEST_SHASUM}

test-eth:  # 💎 Runs Ethereum Foundation tests.
	cargo nextest run --workspace --features ethereum_foundation_tests --no-capture --test ef_tests

###### Benchmarks ######

revm-comparison:
	$(MAKE) build-revm-comparison
	@echo
	@printf "%s" "revm_factorial result: "
	@target/release/revm_factorial 1 1000
	@printf "%s" "levm_factorial result: "
	@target/release/levm_factorial 1 1000
	hyperfine -w 5 -r 10 -N \
		-n "revm_factorial" "target/release/revm_factorial 100000 1000" \
		-n "levm_factorial" "target/release/levm_factorial 100000 1000" 
	@echo
	@printf "%s" "revm_fibonacci result: "
	@target/release/revm_fibonacci 1 1000
	@printf "%s" "levm_fibonacci result: "
	@target/release/levm_fibonacci 1 1000
	hyperfine -w 5 -r 10 -N \
		-n "revm_fibonacci" "target/release/revm_fibonacci 100000 1000" \
		-n "levm_fibonacci" "target/release/levm_fibonacci 100000 1000" 
	@echo

build-revm-comparison:
	cd bench/revm_comparison && \
		CARGO_TARGET_DIR=../../target \
		cargo build --release \
		--bin revm_factorial \
		--bin levm_factorial \
		--bin revm_fibonacci \
		--bin levm_fibonacci 
