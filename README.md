<p align="center">
  <h1 align="center">Cairo VM Codes: Server Code</h1>
</p>
<p align="center">
  <strong><i>An interactive reference to Cairo Virtual Machine</i></strong>
  <img width="1392" alt="cairovm.codes app screenshot" src="https://github.com/walnuthq/cairovm.codes-server/assets/2983746/63c8813d-70ea-4815-ae03-da9e5ee4af32">
</p>

This is the backend source code that runs [cairovm.codes](http://cairovm.codes) web application. Repository with the frontend code can be found [here](https://github.com/walnuthq/cairovm.codes). Below you will find the docs on how to contribute to the project and get it up and running locally for further development.

cairovm.codes is brought to you by [Walnut](https://www.walnut.dev).

## ⚙️ Installation

The app requires the following dependencies:

- [Rust](https://www.rust-lang.org/) = 1.85.0-nightly

## 👩‍💻 Local Development

For contributing to the project, you can quickly get the application running by following these steps:

Clone this repository:

    git clone git@github.com:walnuthq/cairovm.codes-server.git

Install the dependencies:

    make deps

Start up the app and see it running at http://localhost:3000/_ah/warmup

    make run-dev

## 🚀 Deploying

Deployments are handled automatically, as soon as your PR is merged to `main`.

## 🤗 Contributing

For instructions see [cairovm.codes](https://github.com/walnuthq/cairovm.codes)

## License

[MIT](LICENSE)
