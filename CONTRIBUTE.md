# Contributing to Knut

This file is for everyone who works on Knut, human or AI. It explains how the
project thinks, how the code is organised and how it should read.
[DESIGN.md](DESIGN.md) explains *what* Knut does and why it behaves that way;
[docs/dsl.md](docs/dsl.md) is the reference for people writing a config.

## Mental model

Knut is a small smart-home daemon for one household. It controls lights
(Zigbee through zigbee2mqtt, Philips Hue through the bridge), reacts to
sensors, the sun and the clock, and exposes groups and scenes to Apple Home.
The whole home is described in a Ruby DSL; there is no frontend.

Knut is **not** a general home-automation platform: no plugin system, no
web UI, no device types beyond lights and sensors, no cloud services.

Rust and the DSL were chosen because they let people read what the program
does. Code that is correct but hard to follow defeats that choice. Every
change is measured against one question:

> **Can someone who knows Rust or Ruby, but not this project, read it top to
> bottom and understand it without help?**

"Without help" means no AI, no search engine and no author to ask. If a
change only makes sense after a long explanation, the explanation belongs in
the code, or the code should be simpler.

## Architecture

```
config.rb ──> script (mruby + prelude.rb) ──> model ──> engine ──> adapter ──> lights
                                                         ^  │
                                      homekit ───────────┘  └──> store (/var/lib/knut)
```

| Module     | What it does                                                                        |
|------------|-------------------------------------------------------------------------------------|
| `main.rs`  | The `knut` command line: parses arguments, calls the library.                       |
| `lib.rs`   | The library's public API: the commands `check`, `discover`, `set`, `run`, `unpair`. |
| `commands` | What each command does, wiring the modules below together.                          |
| `script`   | Runs the config in mruby, reads the model back, runs scene and trigger blocks.      |
| `model`    | The configured home as plain data, validated as a whole.                            |
| `engine`   | The daemon's core: entity state, auto mode, scenes, triggers.                       |
| `control`  | Turns a requested group or light state into per-device commands (staging, mapping). |
| `adapter`  | Talks to devices: `zigbee` (MQTT), `hue` (CLIP v2).                                 |
| `homekit`  | The HomeKit bridge. Sees only groups, exposed lights, scenes and the Auto switch.   |
| `sun`      | Sun position and daylight, computed locally.                                        |
| `store`    | Persistent state: accessory ids, auto flags.                                        |

### Why a Ruby DSL?

A home configuration is read far more often than it is written, and it is
read by the person who lives there, not by a programmer. Ruby lets the config
read like a description of the home (`room "Kitchen" do … end`), while still
allowing real logic in triggers. The DSL is part of the product: its
readability matters as much as the Rust code's.

### Why does the DSL record a model instead of acting?

Loading the config only builds a description of the home, which Rust reads
back as JSON and validates as a whole. That keeps all checking in one place,
lets `knut check` report every problem at once, and means a config cannot do
anything while it loads. Scene and trigger blocks are stored and run later by
the engine.

### Why mruby with an own C shim?

mruby is small and links statically into the single binary. The `minutus`
crate no longer compiles against current mruby, so `mruby/shim.c` and
`src/script/mruby.rs` wrap the few C functions Knut needs. Keep that boundary
thin: Ruby code belongs in `prelude.rb`, not in C strings.

### Why is `hap` vendored?

The upstream crate is unmaintained and needed fixes for current dependencies
and for mDNS on hosts with several interfaces. Every change to `vendor/hap` is
listed in `vendor/hap/PATCHES.md`.

### Why does HomeKit never talk to adapters?

HomeKit only sees groups, exposed lights, scenes and the Auto switch, and
asks the engine for changes. A second front end (e.g. Matter) could then be
added without touching device code.

## Domain vocabulary

Use these words in code, comments, logs and docs, and only in this sense.

| Term                | Meaning                                                                                            |
|---------------------|----------------------------------------------------------------------------------------------------|
| **Entity**          | Anything with a path that HomeKit or the DSL can control: a light or a group.                      |
| **Path**            | `"Room/Entity"`, the one way to name an entity.                                                    |
| **Need**            | How much artificial light a group should deliver, 0–1 in the DSL, 0–100 as a group value.          |
| **Demand**          | The need before room daylight is subtracted, usually a curve over the day.                         |
| **Daylight**        | How much light a room gets from the sun right now, 0–1.                                            |
| **Stage**           | A set of lights in a group that is switched on together. **Staging** picks how many stages are on. |
| **Mapping**         | A per-light function from the brightness it receives to the brightness it shows.                   |
| **Auto / override** | An entity in auto follows `adapt`; an overridden one keeps what the user set.                      |
| **Adapter**         | A connection to a device system (one Hue bridge, one zigbee2mqtt instance).                        |

## Conventions

### For all code

- **Write for the reader, not for the compiler.** Prefer one obvious step per
  line over a clever expression. A longer, plain version wins over a shorter,
  dense one. *Because the program is read many more times than it is
  written.*
- **Names say what something is in the domain.** `need`, `stage_count`,
  `exposed_lights`, not `n`, `k`, `tmp`, `data`. Short names are fine for
  very short scopes (`|light| light.on`). *Because a good name replaces a
  comment.*
- **Name conditions.** When a condition has more than one part, give it a
  name: `let unchanged = …; if unchanged { … }`, or a predicate method in
  Ruby. *Because the reader then sees the intent before the mechanics.*
- **One level of abstraction per function.** A function either decides
  (calls named steps) or does one step. If you want a separator banner or a
  comment that says "now do X", extract X. As a guide, a function longer than
  about 40 lines should be split. *Because the top of a module should read
  like a summary of it.*
- **Order code top-down.** Public items first, then the helpers they use, in
  the order they are called. *Because "readable top to bottom" is literal.*
- **Comments explain why**, magic numbers and non-obvious constraints. They
  never retell history ("previously…", "changed to…"); that is git's job.
- **Errors tell the user how to fix it.** Name the place (`config.rb:12`),
  what was wrong and what is expected, with an example where it helps.
- **Libraries are used as their authors intended.** Before writing a helper,
  check whether the library already offers it; follow its documented idioms
  instead of working around them.
- **No new dependencies without discussion.** A crate or gem is a long-term
  maintenance cost. Propose it with what it saves and what it costs.
- **English everywhere**: code, comments, logs, docs and examples, including
  room and entity names in example configs and tests.

### Rust

- **Format with `rustfmt` defaults** (`cargo fmt`), and keep `cargo clippy`
  clean. *Because a uniform layout makes diffs show only real changes and
  stops long lines from creeping in.*
- **Visibility shows the role of an item**, and `unreachable_pub` enforces
  it:
  - `pub`: the library's public API, reachable from `lib.rs`. Only what
    `main.rs` needs.
  - `pub(crate)`: a module's interface, used by other modules.
  - no modifier: private to the module.

  Fields and enum variants of a `pub(crate)` type may be plain `pub`; the
  type already limits who can reach them. *Because a reader should see at
  the item itself who may depend on it, without `pub(crate)` on every line.*
- **Every module starts with a `//!` paragraph**: what it is responsible for
  and what it is not. **Every `pub` and `pub(crate)` item has a `///` doc
  comment** that says what it does and what it promises, not how. Private
  items get comments only where the code cannot speak for itself.
  `cargo doc --document-private-items --open` should read like a guide.
- **Prefer a named struct to a long parameter list.** More than about four
  parameters, or several `Option`s of the same type in a row, belong in a
  struct with named fields. *Because `set(path, None, Some(40.0), None, …)`
  cannot be read at the call site.*
- **Keep `match` arms short.** An arm longer than a few lines calls a
  function named after what it handles.
- **Split large modules along their responsibilities**, not by size alone.
  A file over about 500 lines is a reason to look.
- **Tests read like specifications.** Name them after the behaviour
  (`staging_adds_a_stage_above_95_percent`), test one idea each, and keep
  configs as small as the test allows. Longer configs live in
  `tests/fixtures/*.rb`, not in inline strings.

### Ruby (prelude and DSL)

- **The prelude has visible layers**, each in its own section or file:
  1. **DSL**: the top-level verbs a config uses (`room`, `scene`, `on`,
     `set`, …) and the public methods of the builders (`light`, `group`,
     `stages`, …). This is the public API of the language.
  2. **Runtime**: the methods Rust calls (`Knut.model_json`, `Knut.run`,
     `Knut.staging`, `Knut.mapping_table`).
  3. **Internals**: validation, JSON encoding, proc registry. Marked
     `private` or kept in `Knut::Internal`.

  *Because a reader of the prelude should know at once whether a method is
  something a config author may call.*
- **Every DSL method has a comment** with one sentence of purpose, its
  arguments with types and ranges, and a one-line example. The same facts
  appear in `docs/dsl.md`.
- **Guard clauses, one condition each.** Write

  ```ruby
  unless clock_time?(value)
    Knut.fail("#{what} must be an \"HH:MM\" time, got #{value.inspect}")
  end
  ```

  instead of combining several `&&` checks with the error on one line.
- **Small, named validators** (`clock_time?`, `non_empty_array?`) instead of
  inline checks. *Because they read like the rule they enforce.*
- **Idiomatic Ruby within mruby's limits.** mruby has no `Regexp` and no
  JSON library in this build; where that forces a workaround, say so in one
  line.
- **Error messages show correct input**, e.g. `expected an Array of lights
  like [:spot1]`.

### DSL design

- **The DSL is read by the person who lives in the home.** New verbs and
  options use everyday words, not implementation terms.
- **One way to say one thing.** Do not add aliases or alternative spellings.
- **Fail at load time** with a message that names the line and the fix.
- **Every DSL change updates `docs/dsl.md` and `examples/config.rb`** in the
  same change.

## Build reference

```sh
git submodule update --init               # mruby sources
cargo build --release
cargo test
cargo fmt --check && cargo clippy
cargo doc --document-private-items --open
target/release/knut check examples/config.rb
.scratch/homekit/.venv/bin/python .scratch/homekit/run.py   # local HomeKit pairing test
```

Deployment runs through `docker compose` (see README.md).

## Working with AI

### Understand before changing

Read the module's `//!` docs, the relevant part of DESIGN.md and the code
around the change before proposing anything. If something is unclear, ask;
do not guess.

### Explain before implementing

Every change starts with a short plan: what changes, why, and what it
affects. Once the user has approved a plan, carry it out without asking
again for each step. Design changes that the plan did not cover (new
modules, new types, changed interfaces, new DSL syntax) are proposed first.

### What to preserve

- The split DSL → model → engine; the DSL only records, Rust validates.
- HomeKit talks only to the engine, never to adapters.
- Stable HomeKit accessory ids, derived from config names.
- The thin mruby shim; Ruby logic stays in `prelude.rb`.
- `vendor/hap/PATCHES.md` lists every change to `vendor/hap`.

### What to never do

- Refactor or "clean up" code outside the agreed change.
- Add a crate, gem or service without discussion.
- Change DSL syntax without updating `docs/dsl.md` and the example.
- Make an item `pub` to silence a visibility error; decide where it belongs.
- Leave dense one-liners because they work. Readability is part of done.
- Write history into comments.

### How to suggest changes

Disagreement is welcome. If an approach seems wrong, too complex or against
these conventions, say so before implementing, with a concrete alternative.
Silent compliance is worse than a short discussion.

### Use libraries as their authors intended

Ask how the library's author would solve the problem, not only how to make it
work. Prefer documented APIs and idioms over workarounds.
