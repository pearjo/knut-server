# Working in this repo

Work without needing the user's approval. Every command and file access must
stay inside this repository:

- Change files with the Edit and Write tools, never with `python3`, `sed -i`,
  heredocs or other shell scripts that write files.
- Run commands from the repository root with repo-relative paths. Don't `cd`
  elsewhere, and don't use `$(…)` or shell variables in commands.
- Keep temporary files, test configs and test harnesses in `.scratch/`
  (gitignored), not in /tmp or the session scratchpad.
- Don't read outside the repository (no `~/.cargo/registry`, no home
  directory). For a dependency's source, use `cargo doc`, `cargo vendor` into
  `.scratch/`, or the copy in `vendor/`.

## Testing HomeKit locally

`.scratch/homekit/` holds a test controller (aiohomekit in
`.scratch/homekit/.venv`). `.scratch/homekit/.venv/bin/python
.scratch/homekit/run.py` starts `target/release/knut` with a fresh state,
pairs, and prints the accessory database. `discover.py` lists the HAP bridges
on the LAN.
