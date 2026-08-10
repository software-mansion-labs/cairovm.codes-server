.PHONY: deps check fmt fmt-check clippy lint

deps:
	git clone https://github.com/starkware-libs/cairo.git \
	&& cd cairo \
	&& git checkout 93221753088d58f54f9a7f35a0bb338cf0bfb952 \
	&& cd .. \
	&& mv cairo/corelib/ . \
	&& rm -rf cairo/

# Type-check the whole workspace, including tests and benches
check:
	cargo check --workspace --all-targets --locked

# Format the code in place
fmt:
	cargo fmt --all

# Verify formatting without modifying files (used by CI)
fmt-check:
	cargo fmt --all -- --check

# Lint with clippy, treating warnings as errors
clippy:
	cargo clippy --workspace --all-targets --locked -- -D warnings

# Everything CI runs: formatting + lints
lint: fmt-check clippy
