ARG RUST_VERSION=1.80.0

FROM public.ecr.aws/docker/library/rust:${RUST_VERSION} AS builder
WORKDIR /app
COPY . .
RUN make deps
RUN cargo build --locked --release --bin server

FROM public.ecr.aws/docker/library/rust:${RUST_VERSION} AS final
RUN adduser \
  --disabled-password \
  --gecos "" \
  --home "/nonexistent" \
  --shell "/sbin/nologin" \
  --no-create-home \
  --uid "10001" \
  appuser
COPY --from=builder /app/target/release/server /opt/app/server
COPY --from=builder /app/corelib /opt/corelib
RUN chown -R appuser /opt/app
USER appuser
WORKDIR /opt/app
EXPOSE 3000
ENTRYPOINT ["/opt/app/server"]
