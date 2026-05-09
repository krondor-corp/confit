.PHONY: check install wiki

PORT ?= 4000

check:
	cargo build
	cargo test
	cargo clippy -- -D warnings
	cargo fmt --check

install:
	cargo install --path crates/confit-cli

wiki:
	cd wiki && bundle exec jekyll serve --livereload --port $(PORT)
