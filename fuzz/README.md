# Fuzz Testing (`fuzz/`)

`cargo-fuzz` (libFuzzer) harnesses for every parser that handles **untrusted
input**. They complement the property-based tests: the property tests cover
invariants, while these targets hunt for inputs that crash, panic, or hang a
parser. Each harness feeds randomized input into a single parser/validator and
never touches real resources (no network, no filesystem writes, no process
execution) — satisfying Requirements 38.1 and 38.2.

## Prerequisites

`cargo-fuzz` builds with libFuzzer, which requires a **nightly** toolchain (the
repository pins stable `1.82.0` for normal builds).

```bash
# One-time install of the cargo subcommand.
cargo install cargo-fuzz

# Install a nightly toolchain for the fuzzers (libFuzzer needs nightly).
rustup toolchain install nightly
```

## Available targets

Run `cargo fuzz list` to enumerate targets, or pick one from the table below.

| Target              | Parser under test                              | Requirement |
|---------------------|------------------------------------------------|-------------|
| `fuzz_config`       | TOML configuration parser (`cyrene_config::Config::parse`) | 38.1 |
| `fuzz_tool_params`  | Tool-parameter / `ToolCall` argument parsing   | 38.1 |
| `fuzz_json_payload` | JSON payload parsing (webhook + model-provider responses) | 38.1 |
| `fuzz_skill_parse`  | `SKILL.md` parser (`cyrene_skills::Skill::from_skill_md`) | 38.1 |

## Running a target

The documented command (Requirement 38.3) is:

```bash
cargo +nightly fuzz run <target>
```

For example:

```bash
cargo +nightly fuzz run fuzz_config
cargo +nightly fuzz run fuzz_tool_params
cargo +nightly fuzz run fuzz_json_payload
cargo +nightly fuzz run fuzz_skill_parse
```

Useful flags (passed through to libFuzzer after `--`):

```bash
# Stop after N seconds (handy in CI smoke runs).
cargo +nightly fuzz run fuzz_config -- -max_total_time=60

# Bound a single input's size.
cargo +nightly fuzz run fuzz_json_payload -- -max_len=4096
```

`cargo fuzz run <target>` automatically replays everything already stored in
`corpus/<target>/` before it starts exploring new inputs, so committed
regression cases are exercised on every run.

## Retained regression corpus (Requirement 38.4)

Layout:

```
fuzz/
├── corpus/            # COMMITTED — retained seeds + crash reproducers (replayed every run)
│   ├── fuzz_config/
│   ├── fuzz_tool_params/
│   ├── fuzz_json_payload/
│   └── fuzz_skill_parse/
└── artifacts/         # IGNORED — transient crash/timeout/oom dumps written on failure
```

- `corpus/<target>/` is **checked into git** (each directory keeps a `.gitkeep`
  so the structure persists even while empty). This is the durable regression
  corpus that `cargo fuzz run` replays on every invocation.
- `artifacts/` and `target/` are **git-ignored** because they are transient
  build output and raw crash dumps.

### How a crashing input becomes a retained regression test

When a target discovers an input that crashes the parser, libFuzzer writes the
reproducing bytes to `fuzz/artifacts/<target>/crash-<hash>` and prints the path.
To turn that crash into a permanent regression case:

```bash
# 1. (Optional) Minimize the reproducer to the smallest crashing input.
cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/crash-<hash>

# 2. Confirm it reproduces.
cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-<hash>

# 3. Promote it into the retained corpus and commit it.
cp fuzz/artifacts/<target>/crash-<hash> fuzz/corpus/<target>/
git add fuzz/corpus/<target>/
```

Once committed, the reproducer lives in `corpus/<target>/` and is replayed by
every subsequent `cargo fuzz run <target>`, so the bug stays fixed (the run
fails again if a regression reintroduces the crash). Fix the underlying parser,
then keep the reproducer in the corpus as the guard against regressions.
