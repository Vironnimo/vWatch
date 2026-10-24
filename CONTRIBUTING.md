# Contributing

Thanks for your interest in vWatch. It is a small project with one maintainer,
so please open an issue before you start on a larger change. Bug reports with
clear steps to reproduce help the most.

## Building and testing

vWatch needs Windows 11, Git, the Rust toolchain pinned in `rust-toolchain.toml`,
the MSVC build tools, and the Python and Node versions in `.python-version` and
`.node-version`. There is no npm install and no frontend build step.

```powershell
rustup toolchain install 1.97.1 --profile minimal --component rustfmt --component clippy
git config --local core.hooksPath .githooks
python scripts/check.py tools
```

Run the window from source against a scratch data folder, so your installed
vWatch data stays untouched:

```powershell
$env:VWATCH_HOME = "$PWD\.tmp\dev-instance"
cargo run
```

```powershell
python scripts/test.py
python scripts/check.py all
```

The first runs the Rust and UI tests; the second runs every check, including
formatting, Clippy and the core build without the window. The commit hook runs
the static checks on every commit.

[AGENTS.md](AGENTS.md) holds the repository conventions that coding agents and
people follow alike, and `.vorch/PROJECT.md` maps the architecture and the
documentation of each area.

## Pull requests

- Keep one logical change per pull request and add or extend a test for the
  behavior you change.
- Write commit messages in the Conventional format, for example
  `fix(facts): keep incomplete measurements unknown`.
- The window's text is German. If German is a hurdle, write the English text in
  the pull request and the maintainer will take care of it.
- Changes to CLI commands, output or document schemas also update the operating
  Skill in `skill/vwatch/` and `docs/agent-interface.md`.

By contributing, you agree that your contribution is licensed under the
[MIT License](LICENSE).
