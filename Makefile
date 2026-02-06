build:
	@cargo build

test:
	@cargo nextest run --all-features

test-unit:
	@cargo test -p ghc-core -p ghc-api -p ghc-git -p ghc-cmd

test-clippy:
	@cargo clippy -- -D warnings

test-fmt:
	@cargo +nightly fmt -- --check

check: test-fmt test-clippy test-unit

parity-test:
	@cargo test -p ghc --test parity -- --ignored

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test test-unit test-clippy test-fmt check parity-test release update-submodule
