<p align="center">
  <h1 align="center">Cairo VM Codes: Server Code</h1>
</p>
<p align="center">
  <strong><i>An interactive reference to Cairo Virtual Machine</i></strong>
  <img width="1392" alt="cairovm.codes app screenshot" src="https://github.com/walnuthq/cairovm.codes-server/assets/2983746/63c8813d-70ea-4815-ae03-da9e5ee4af32">
</p>

This is the backend source code that runs [cairovm.codes](http://cairovm.codes) web application. Repository with the frontend code can be found [here](https://github.com/walnuthq/cairovm.codes). Below you will find the docs on how to contribute to the project and get it up and running locally for further development.

cairovm.codes is brought to you by [Walnut](https://www.walnut.dev).

## 🏗 Architecture

The repository contains two Rust crates:

- [`server/`](server/) — an axum web server exposing a WebSocket endpoint (`/ws`) that compiles and runs Cairo programs, streaming back the execution trace, Sierra/CASM mappings and (optionally) a STARK proof
- [`prover/`](prover/) — a standalone binary wrapping the [stwo](https://github.com/starkware-libs/stwo-cairo) prover; the server invokes it via the `PROVER_PATH` environment variable

### Endpoints

| Route | Description |
|---|---|
| `GET /ws` | WebSocket. Send a JSON `RunnerPayload` (`cairo_program_code`, optional `program_arguments`, `proof_required`, `verification_required`); receive `RunnerResult` and, if requested, `ProverResult` messages |
| `GET /health` | Health check, returns `200` |
| `GET /_ah/warmup` | Warmup probe, returns `OK` |

### Environment variables

| Variable | Required | Description |
|---|---|---|
| `PROVER_PATH` | For proving only | Path to the `prover` binary. Without it, runs still work but proving requests fail |

## ⚙️ Installation

The app requires the following dependencies:

- [Rust](https://www.rust-lang.org/) — the pinned nightly toolchain is installed automatically via `rust-toolchain.toml`

## 👩‍💻 Local Development

For contributing to the project, you can quickly get the application running by following these steps:

Clone this repository:

    git clone git@github.com:walnuthq/cairovm.codes-server.git

Install the dependencies:

    make deps

Start up the app (builds the prover, then runs the server with `PROVER_PATH` set) and see it running at http://localhost:3000/_ah/warmup

    make run-dev

Before opening a PR, make sure the code is formatted and lint-clean:

    make fmt
    make lint

## 🚀 Deploying

The server is deployed with Docker Compose (see [`docker-compose.yaml`](docker-compose.yaml)): an `app` container built from the [`Dockerfile`](Dockerfile) behind an `nginx` reverse proxy that terminates TLS and rate-limits requests.

## 🤗 Contributing

For instructions see [cairovm.codes](https://github.com/walnuthq/cairovm.codes)

## License

[MIT](LICENSE)
