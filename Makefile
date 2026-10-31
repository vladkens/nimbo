.PHONY: prepare check test build update clean

prepare:
	cargo +nightly fmt
	uv run scripts/blank-lines.py --fix
	cargo clippy --all-targets --fix --allow-dirty --allow-staged

check: check-targets
	cargo +nightly fmt --check
	uv run scripts/blank-lines.py
	uv run --no-project python -m doctest scripts/pace.py scripts/size.py scripts/macos-min.py
	uv run scripts/size.py --check
	uv run scripts/macos-min.py
	cargo clippy --all-targets -- -D warnings
	if [ "$$(uname)" = Darwin ]; then cargo clippy --all-targets --features metal -- -D warnings; fi

test:
	cargo test
	if [ "$$(uname)" = Darwin ]; then cargo test --features metal metal; fi

build:
	cargo build --release --examples

update:
	cargo upgrade --incompatible
	cargo update

clean:
	cargo clean

.PHONY: check-targets size pace

# The library, its examples and tests build for these too; needs `rustup target add` of each
# (and of aarch64-apple-darwin off a Mac).
TARGETS := x86_64-pc-windows-msvc x86_64-unknown-linux-gnu

check-targets: # clippy of the library, examples and tests for Windows and Linux, off a Mac for macOS with Metal
	@for t in $(TARGETS); do cargo clippy --all-targets --target $$t -- -D warnings || exit 1; done
	@if [ "$$(uname)" != Darwin ]; then cargo clippy --all-targets --target aarch64-apple-darwin --features metal -- -D warnings; fi

pace: # frame pacing in a window of this session: fps while animating, wakeups when still
	cargo build --release --example pace
	uv run scripts/pace.py

size: # code, docs and tests per module; ARGS="--files", "--map" (each file and what it does), "--check" (the budgets) or "--vs main"
	uv run scripts/size.py $(ARGS)
