ARG RUST_VERSION=nightly-2025-01-02

FROM public.ecr.aws/docker/library/rust:1.85.0 AS builder
RUN rustup install ${RUST_VERSION}
RUN rustup default ${RUST_VERSION}
WORKDIR /app
COPY . .
RUN make deps
RUN cd prover && cargo build --locked --release --bin prover
RUN cd server && cargo build --locked --release --bin server

FROM public.ecr.aws/docker/library/rust:1.85.0 AS final
RUN rustup install ${RUST_VERSION}
RUN rustup default ${RUST_VERSION}
RUN adduser \
  --disabled-password \
  --gecos "" \
  --home "/nonexistent" \
  --shell "/sbin/nologin" \
  --no-create-home \
  --uid "10001" \
  appuser
COPY --from=builder /app/prover/target/release/prover /opt/app/prover
ENV PROVER_PATH=/opt/app/prover
COPY --from=builder /app/server/target/release/server /opt/app/server
COPY --from=builder /app/corelib /opt/corelib
RUN chown -R appuser /opt/app
USER appuser
WORKDIR /opt/app
EXPOSE 3000
ENTRYPOINT ["/opt/app/server"]
