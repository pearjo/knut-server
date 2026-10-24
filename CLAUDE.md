Read [CONTRIBUTE.md](CONTRIBUTE.md) first. It holds the project's mental model,
conventions and working agreements; this file only adds what is specific to
the AI tooling.

## Quick reference

```sh
cargo build --release
cargo test
cargo fmt --check && cargo clippy
target/release/knut check examples/config.rb
.scratch/homekit/.venv/bin/python .scratch/homekit/run.py
```

`run.py` starts `target/release/knut` with a fresh state, pairs with the test
controller in `.scratch/homekit/.venv` and prints the accessory database.
`discover.py` lists the HAP bridges on the LAN. `hub.py` plays a home hub:
it adds and removes a second controller and connects as a known and an
unknown controller, printing the ERROR lines Knut logs after each step.
Run the vendored hap's tests with `cargo test -p hap` (it needs Knut's
tokio features).

## Pre-flight checklist

1. Can someone who knows Rust or Ruby, but not Knut, read the change top to
   bottom and understand it without help?
2. Does it change a design decision, a module interface or DSL syntax that
   the approved plan did not cover? If yes: propose it and wait.
3. Does it add a dependency? If yes: justify it and wait.

## Tool use without permission prompts

Once a plan is approved, work through it without asking for approval of
single steps. Every command and file access must stay inside this repository:

- Change files with the Edit and Write tools, never with `python3`, `sed -i`,
  heredocs or other shell scripts that write files.
- Run commands from the repository root with repo-relative paths. Don't `cd`
  elsewhere, and don't use `$(…)` or shell variables in commands.
- Keep temporary files, test configs and test harnesses in `.scratch/`
  (gitignored), not in /tmp or the session scratchpad.
- Don't read outside the repository (no `~/.cargo/registry`, no home
  directory). For a dependency's source, use `cargo doc`, `cargo vendor` into
  `.scratch/`, or the copy in `vendor/`.
