# @cognelis/signoz-cli

Read-only SigNoz queries, connection diagnostics, named environments, and shell
completion for terminals, scripts, and coding agents.

```sh
npm install -g @cognelis/signoz-cli
signoz-cli version --output json
signoz-cli init
```

The npm package installs a prebuilt native executable. No Rust toolchain is
required. It supports macOS 12 or newer, Linux with glibc 2.35 or newer and
`libdbus-1`, and Windows on x64 and ARM64. Alpine/musl requires a source build.
Persistent Linux credentials also require an available Secret Service; CI jobs
can use `SIGNOZ_URL` and `SIGNOZ_API_KEY` from their secret store. Installation
downloads the matching release binary and requires a valid SHA-256 checksum;
network and integrity failures are reported instead of running an unverified file.
If lifecycle scripts are disabled, the launcher retries installation on first use.

```sh
signoz-cli --profile prod doctor --output json
# Continue after doctor succeeds.
signoz-cli --profile prod logs search --since 1h --output json
signoz-cli skills install
```

For zsh completion in the current shell:

```zsh
autoload -Uz compinit && compinit
source <(signoz-cli completion zsh)
```

Human-readable output is the default on a terminal. Use `--output json` for scripts
and agents, and inspect the exit code and pagination metadata. The launcher passes
arguments, standard streams, and process exit status through to the Rust CLI.
