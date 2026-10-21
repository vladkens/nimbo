.PHONY: prepare check test build update clean

prepare:
	cargo +nightly fmt
	cargo clippy --all-targets --fix --allow-dirty --allow-staged

check: check-targets
	cargo +nightly fmt --check
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

build:
	cargo build --release --examples

update:
	cargo upgrade --incompatible
	cargo update

clean:
	cargo clean

.PHONY: check-targets

# The library, its examples and tests build for these too; needs `rustup target add` of each.
TARGETS := x86_64-pc-windows-msvc x86_64-unknown-linux-gnu

check-targets: # clippy of the library, examples and tests for Windows and Linux
	@for t in $(TARGETS); do cargo clippy --all-targets --target $$t -- -D warnings || exit 1; done
