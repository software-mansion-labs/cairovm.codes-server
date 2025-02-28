deps:
	git clone https://github.com/starkware-libs/cairo.git \
	&& cd cairo \
	&& git checkout c539d077479654eee6323d9c0c6eafad82d4851a \
	&& cd .. \
	&& mv cairo/corelib/ . \
	&& rm -rf cairo/

run-dev:
	cd prover && cargo build --release \
	&& cp target/release/prover ./../prover-bin \
	&& export PROVER_PATH="../prover-bin" && cd .. && cd server && cargo run --release