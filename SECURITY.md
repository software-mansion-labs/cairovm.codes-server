# Security Policy

## Reporting a Vulnerability

Please **do not** report security vulnerabilities through public GitHub issues.

Instead, report them privately via one of the following channels:

- **GitHub private vulnerability reporting**: use the
  ["Report a vulnerability"](https://github.com/walnuthq/cairovm.codes-server/security/advisories/new)
  form on this repository.
- **Email**: hi@walnut.dev

Please include as much of the following as you can:

- A description of the issue and its impact
- Steps to reproduce (a minimal Cairo program or HTTP request is ideal)
- The affected endpoint or component

We will acknowledge your report within a few business days and keep you
informed of the progress towards a fix. Please give us a reasonable amount of
time to address the issue before any public disclosure.

## Scope

This service compiles and executes untrusted Cairo programs submitted over
HTTP. Reports we are particularly interested in include:

- Sandbox escapes: executing a Cairo program that reads or writes files,
  reaches the network, or otherwise affects the host beyond its own execution
- Remote code execution or memory-safety issues in the server itself
- Denial-of-service vectors that a rate limiter cannot reasonably mitigate
- Information disclosure (e.g. leaking data from other users' requests)

## Supported Versions

Only the latest code on the `main` branch (what is deployed to
[cairovm.codes](https://cairovm.codes)) is supported.
