.PHONY: prepare check test build update clean

LINT := uv run scripts/lint.py

prepare:
	cargo +nightly fmt
	$(LINT) fix
	cargo clippy --all-targets --fix --allow-dirty --allow-staged

check: check-targets check-api
	cargo +nightly fmt --check
	uv run --no-project python -m doctest scripts/pace.py scripts/macos-min.py
	$(LINT) size --check
	uv run scripts/macos-min.py
	cargo clippy --all-targets -- -D warnings
	$(LINT)

test:
	cargo test

build:
	cargo build --release --examples

update:
	cargo upgrade --incompatible
	cargo update

clean:
	cargo clean

.PHONY: check-targets check-api public-api features size pace api-shots i18n

# The library, its examples and tests build for these too; needs `rustup target add` of each.
TARGETS := x86_64-pc-windows-msvc x86_64-unknown-linux-gnu

check-targets: # clippy of the library, examples and tests for Windows and Linux
	@for t in $(TARGETS); do cargo clippy --all-targets --target $$t -- -D warnings || exit 1; done

# The public API is the same on every system: one listing, checked on each. Rustdoc JSON of a
# pinned nightly, as its paths change between nightlies; blanket and auto-trait impls left out.
# Needs cargo-public-api (`cargo install cargo-public-api --locked`) and the nightly with the
# targets (`rustup toolchain install $(API_NIGHTLY) --profile minimal --target <each>`).
API_NIGHTLY := nightly-2026-11-02
PUBLIC_API := cargo +$(API_NIGHTLY) public-api --all-features -ss --color never --target

check-api: # the public API with every feature on each system against docs/public-api.txt
	@for t in aarch64-apple-darwin $(TARGETS); do \
	  $(PUBLIC_API) $$t | diff -u docs/public-api.txt - \
	    || { echo "the public API on $$t is not docs/public-api.txt: make public-api"; exit 1; }; \
	done

public-api: # docs/public-api.txt, the public API with every feature, from rustdoc
	mkdir -p target && $(PUBLIC_API) aarch64-apple-darwin > target/public-api.txt && mv target/public-api.txt docs/public-api.txt

# Its own target dir: warnings are errors here, and the flag would rebuild the usual one's crates.
features: # the library with each feature alone, none and the default, on every system; then the base and headless built; needs cargo-hack (`cargo install cargo-hack`)
	@for t in aarch64-apple-darwin $(TARGETS); do \
	  RUSTFLAGS="-D warnings" CARGO_TARGET_DIR=target/features cargo hack check --lib --each-feature --target $$t || exit 1; \
	done
	RUSTFLAGS="-D warnings" CARGO_TARGET_DIR=target/features cargo build --lib --no-default-features
	RUSTFLAGS="-D warnings" CARGO_TARGET_DIR=target/features cargo build --lib --no-default-features --features headless

pace: # frame pacing in a window of this session: fps while animating, wakeups when still
	cargo build --release --example canvas
	uv run scripts/pace.py

size: # code, docs and tests per module; ARGS="--files", "--map" (each file and what it does), "--check" (at most 600 lines of library code added since main) or "--vs main"
	$(LINT) size $(ARGS)

api-shots: # docs/api.md and its pictures (into the assets branch's worktree), from the gallery's table
	uv run scripts/api-shots.py

# GNU gettext 0.24 or later (`brew install gettext`), the first to read Rust; the locale module's
# own strings are its tests'.
i18n: # nimbo's own strings: locales/nimbo.pot from the code, merged into each language's .po
	xgettext --language=Rust --keyword='t!:1' --keyword='tn!:1,2' --add-comments=Translators --no-location \
	  --from-code=UTF-8 --package-name=nimbo -o locales/nimbo.pot \
	  $$(find src -name '*.rs' -not -path 'src/locale/*' | sort)
	for po in locales/*.po; do msgmerge -q --update --backup=none --no-location $$po locales/nimbo.pot; done
	for po in locales/*.po; do msgfmt -c --statistics -o /dev/null $$po || exit 1; done
