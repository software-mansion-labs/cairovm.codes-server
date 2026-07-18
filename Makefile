.PHONY: deps run-dev check fmt fmt-check clippy lint

deps:
	git clone https://github.com/starkware-libs/cairo.git \
	&& cd cairo \
	&& git checkout c539d077479654eee6323d9c0c6eafad82d4851a \
	&& cd .. \
	&& mv cairo/corelib/ . \
	&& rm -rf cairo/

run-dev:
	cd prover && RUSTFLAGS="-C target-cpu=native -C opt-level=3" cargo build --release \
	&& cp target/release/prover ./../prover-bin \
	&& export PROVER_PATH="../prover-bin" && cd .. && cd server && cargo run --release

# Type-check both crates, including tests and benches
check:
	cd server && cargo check --all-targets --locked
	cd prover && cargo check --all-targets --locked

# Format the code in place
fmt:
	cd server && cargo fmt --all
	cd prover && cargo fmt --all

# Verify formatting without modifying files (used by CI)
fmt-check:
	cd server && cargo fmt --all -- --check
	cd prover && cargo fmt --all -- --check

# Lint with clippy, treating warnings as errors
clippy:
	cd server && cargo clippy --all-targets --locked -- -D warnings
	cd prover && cargo clippy --all-targets --locked -- -D warnings

# Everything CI runs: formatting + lints
lint: fmt-check clippy
