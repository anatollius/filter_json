# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`filter_json` is a Python extension module written in Rust using [PyO3](https://pyo3.rs/) and [Maturin](https://www.maturin.rs/). Rust functions are exposed directly to Python via `#[pyfunction]` and `#[pymodule]` macros.

It is a tool for for transforming JSON text by filtering it. The way that it differs from other tools is that it avoids fully deserializing data while performing the filtering, leading to greater efficiencies.

Users provide their JSON stream and provide a set of inclusion criteria in the form of a set of key names and the tool will stream out a JSON object that only includes the items that fit the inclusion criteria. Alternatively, users can supply exclusion criteria which filter out any keys that match the criteria.

## Commands

**Development build and install into current Python environment:**
```bash
maturin develop
```

**Release build (produces wheel in `target/wheels/`):**
```bash
maturin build --release
```

**Lint:**
```bash
cargo clippy
```

**Run Rust tests:**
```bash
cargo test
```

**Format Rust code:**
```bash
cargo fmt
```

## Architecture

All logic lives in `src/lib.rs`. The module structure uses PyO3's inline module syntax:

```rust
#[pymodule]
mod filter_json {
    #[pyfunction]
    fn my_function(...) -> PyResult<...> { ... }
}
```

The compiled `.so`/`.pyd` file is the Python package — there is no separate Python source. After `maturin develop`, the module is importable as `import filter_json`.

The CI pipeline (`.github/workflows/CI.yml`) builds wheels for Linux (glibc + musl), Windows, and macOS across multiple architectures, then publishes to PyPI on tag push.
