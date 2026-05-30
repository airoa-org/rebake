# Contributing to rebake

Thanks for considering a contribution! rebake is in its early days (pre-1.0), and bug reports, fixes, and well-scoped improvements are all welcome.

## Code of Conduct

This project adheres to the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to uphold it. To report concerns privately, contact the maintainers at `report@airoa.org`.

## Reporting issues

- **Bugs**: open a GitHub issue with a minimal reproducible example (the rosbag or pipeline config that triggers it), the rebake version, and your environment (OS, GPU/codec setup, FFmpeg version if relevant).
- **Security issues**: please report privately to `report@airoa.org` rather than opening a public issue.
- **Feature ideas**: open an issue to discuss before sending a large PR. Small fixes can go straight to a PR.

## Finding ways to help

Issues labeled [`good first issue`](https://github.com/airoa-org/rebake/labels/good%20first%20issue) and [`help wanted`](https://github.com/airoa-org/rebake/labels/help%20wanted) are good starting points.

rebake is pre-1.0 with a deliberately narrow public surface. Before opening a PR for new functionality, please open an issue to discuss it — we may decide the feature is out of scope or needs a different shape.

## Development setup

### Clone

```bash
git clone --recursive https://github.com/airoa-org/rebake.git
cd rebake
```

The repository uses git submodules; `--recursive` is required.

### Docker (recommended)

The dev container has all toolchains preinstalled:

```bash
cd docker
docker compose up -d --build
docker compose exec rebake-dev bash
```

For GPU-accelerated codecs (optional), see [docs/hardware.md](docs/hardware.md).

### Local toolchain

If you prefer to develop outside Docker:

- **Rust**: edition 2024, minimum supported Rust version **1.88** (see [`Cargo.toml`](Cargo.toml)).
- **Python**: 3.9+ (the package ships as an `abi3-py39` wheel).
- **FFmpeg**: required for the video stages.

For Python, install with [uv](https://github.com/astral-sh/uv):

```bash
cd python
uv sync
uv run maturin develop  # build the PyO3 bindings into the venv
```

## Building and testing

### Rust

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

### Python

```bash
cd python
uv run pytest tests/unit/
uv run ruff check rebake/
```

## Code style

- **Rust**: `rustfmt` and `clippy` settings are enforced by the workspace. `clippy` denies correctness lints, warns on suspicious / complexity / perf / style, and warns on `unwrap`, `expect`, `todo`, `dbg`.
- **Python**: `ruff` for linting, `black` style for formatting.

## Pull requests

- Branch from `main`.
- Run the relevant tests and lints locally before opening the PR.
- Write a clear PR description: what changed, why, and a brief test plan.
- Commits in this repository follow [Conventional Commits](https://www.conventionalcommits.org/) (e.g. `feat:`, `fix:`, `docs:`, `chore:`, `ci:`).
- Keep changes focused — unrelated changes are easier to review as separate PRs.
- All contributions are licensed under [Apache-2.0](LICENSE).

## Use of AI

Contributors may use a variety of tools when preparing changes to rebake, including AI systems (e.g. large language models or coding assistants). Contributors using such systems are expected to follow these principles:

- Regardless of how a change is produced, the individual submitting the pull request is considered the **author** of the contribution and is fully **responsible** for it.
- The pull request author **must understand the implementation end-to-end** and be able to **explain and justify the design and code** during review.
- Tools, including AI systems, **are not** considered contributors. **Responsibility and authorship remain with the human** submitting the change.
- Contributors are **encouraged to disclose** significant AI assistance in the pull request description for transparency.
- AI-generated code must be tested in your own environment — do not submit code for a platform or codec backend (e.g. VA-API or NVENC hardware paths) that you cannot run locally.

## Documentation

- `README.md` and top-level `docs/*.md` are maintained as English + Japanese mirrors (`*_ja.md`). If you update one, please update the other in the same change.
- `python/docs/**` (Python API reference) is English-only.

## Need help?

If you get stuck or want to discuss before starting, please open an issue or start a [GitHub Discussion](https://github.com/airoa-org/rebake/discussions).

---

Thank you for contributing to rebake! 🤖
