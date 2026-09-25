# Luminatti

Luminatti is a Rust tool for reviewing Git and Jujutsu (jj) changes. It includes a terminal diff viewer, optional native desktop reviewer, and AI-assisted commands for explaining changes, drafting commit messages, and generating Git commands.

## What it does

- Compare working tree changes, staged changes, commits, and ref ranges.
- Review GitHub pull requests from the terminal, including the PR for the current branch.
- Navigate diffs side by side, search, filter files, watch for updates, and review stacked commits.
- Add and export inline review annotations from the terminal diff viewer.
- Use Git or jj repositories; Luminatti detects the repository type or accepts an explicit override.
- Optionally use a native desktop review workspace with a file tree, diff tabs, review comments, and a dependency view for JavaScript and TypeScript changes.
- Ask a configured AI provider to explain a change, draft a commit message from staged changes, or propose a Git command.

The desktop workspace is separate from the default terminal build and must be compiled with the `desktop` feature. The TypeScript language server integration is optional; install `vtsls` or `typescript-language-server` in the repository or on `PATH` to enable live symbols and reference counts. The desktop dependency view uses local source analysis and does not require a language server.

## Install

Install the command line application from a checkout with Rust and Cargo:

```sh
cargo install --path . --locked
```

Or download a release binary from [GitHub Releases](https://github.com/jteso/luminatti/releases). macOS desktop app archives are published with releases.

To run the native desktop workspace from a checkout:

```sh
cargo run --features desktop -- desktop
```

The repository also includes `./dev.sh` for running the desktop app locally and `./build.sh` for installing the command line application.

## Requirements

- Git, or jj when working in a Jujutsu repository.
- An AI provider and its credentials for `explain`, `draft`, `operate`, and `configure` workflows that call a model.
- `gh` authenticated with GitHub for pull request review features.
- `fzf` for `luminatti explain --list`.
- `vtsls` or `typescript-language-server` is optional and used only for live TypeScript analysis in the desktop workspace.

## Commands

```sh
# Review uncommitted changes
luminatti diff

# Compare commits, branches, or a range with the working tree
luminatti diff HEAD~1
luminatti diff main..feature
luminatti diff main..-

# Review a GitHub pull request by number, URL, or current branch
luminatti diff --pr 123
luminatti diff https://github.com/owner/repo/pull/123
luminatti diff --detect-pr

# Filter, watch, wrap lines, or review a commit stack
luminatti diff --file src/main.rs --file src/lib.rs
luminatti diff --watch --wrap
luminatti diff main..feature --stacked

# Explain the working tree, staged changes, a commit, or a range
luminatti explain
luminatti explain --staged
luminatti explain HEAD
luminatti explain main..feature
luminatti explain --query "What could break if this changes?"
luminatti explain --list

# Draft a commit message from staged changes
luminatti draft
luminatti draft --context "mention the compatibility fix"

# Generate and review a Git command from a request
luminatti operate "squash the last 3 commits into one"

# Configure a provider interactively
luminatti configure

# Open the native desktop reviewer (requires a desktop-enabled build)
luminatti desktop
```

Run `luminatti --help` or `luminatti <command> --help` for the installed version's options.

## Terminal diff viewer

The terminal viewer presents old and new file contents side by side. It supports syntax highlighting, file filtering, search, watch mode, soft wrapping, and stacked commit navigation. Use the key shown in the viewer to open its help; common navigation keys include `j`/`k` or the arrow keys, `Tab` for the sidebar, `q` to quit, and `?` for keybindings.

Annotations can be added to a selection, hunk, or file. They can be reviewed, edited, removed, copied, or exported. In coding-agent workflows, pressing `s` opens a confirmation; confirming writes the formatted annotations to stdout for use as a follow-up prompt.

Themes include `dark`, `light`, `catppuccin-mocha`, `catppuccin-latte`, `dracula`, `nord`, `one-dark`, `gruvbox-dark`, `gruvbox-light`, `solarized-dark`, `solarized-light`, `flexoki-dark`, and `flexoki-light`. Set a theme with `--theme` or the `LUMINATTI_THEME` environment variable.

## Native desktop reviewer

The desktop workspace opens a repository in a native window. It provides a changed-file navigator, side-by-side diffs, tabs, filters, theme selection, and session review comments. Comments are held in memory for the session and can be copied as a review handoff.

Its Radar view maps JavaScript and TypeScript imports and local function calls around changed files. It can show unchanged files that connect changes, function-level change counts, and resolved relationships. Analysis is local. An optional helper in `tools/radar-layout` improves graph layout; without it, the built-in layout remains available. The helper can be built with `bash scripts/build-radar-layout.sh` and Go.

## AI providers and configuration

Supported providers are OpenAI, Anthropic Claude, Google Gemini, Groq, Ollama, OpenCode Zen, OpenRouter, DeepSeek, xAI, and Vercel AI Gateway. Model availability and names depend on the provider; use a model supported by the selected service. Ollama can run models locally.

Configure interactively:

```sh
luminatti configure
```

Or set environment variables:

```sh
export LUMINATTI_AI_PROVIDER="openai"
export LUMINATTI_API_KEY="your-api-key"
export LUMINATTI_AI_MODEL="your-model-name"
```

Configuration is read from `~/.config/luminatti/luminatti.config.json` by default. Use `--config path/to/file.json` to select a file. Command-line provider, key, and model options override configured values.

```json
{
  "provider": "openai",
  "model": "your-model-name",
  "api_key": "your-api-key",
  "theme": "catppuccin-mocha",
  "wrap": true
}
```

The configuration file may also include `draft.commit_types` to customize commit message prefixes and descriptions. Keep API keys private; prefer environment variables or a local untracked config file.

## Development and releases

```sh
cargo install --path . --locked
./dev.sh
./build.sh
```

`./release.sh <version>` prepares and publishes a versioned release. Pushing a `vX.Y.Z` tag starts the release workflow, which builds the release artifacts. The optional Radar helper source and license notices are in [`tools/radar-layout`](tools/radar-layout).

## License

Luminatti is licensed under the [MIT License](LICENSE). The optional Radar layout helper includes D2 TALA components; see [`tools/radar-layout/NOTICE.md`](tools/radar-layout/NOTICE.md) and the accompanying license files.
