# luminatti

A fast terminal diff viewer and code review TUI, written in Rust.

[![GitHub Releases](https://img.shields.io/github/downloads/jteso/luminatti/total?label=dowloads%20%40releases)](https://github.com/jteso/luminatti/releases)
![GitHub License](https://img.shields.io/github/license/jteso/luminatti)

Review `git diff`, commits, branches, or GitHub PRs side-by-side without leaving your terminal. Ships as a single static Rust binary and stays snappy on multi-thousand-line diffs.

- Side-by-side diff viewer with tree-sitter syntax highlighting
- Review GitHub Pull Requests with `luminatti diff --pr 123`
- Annotate selections, hunks, or whole files
- Watch mode and stacked-commit review
- Optional AI commit messages and change explanations (10+ providers)
- Works with Git and Jujutsu (jj)

[![Demo](https://github.com/user-attachments/assets/dc425871-3826-4368-88d8-931b9403f0ec)](https://github.com/user-attachments/assets/70d07324-8394-423c-bbc3-9460ed84877b)

## Table of Contents
- [Getting Started](#getting-started-)
  - [Prerequisites](#prerequisites)
  - [Installation](#installation)
- [Local development and releases](#local-development-and-releases)
- [Usage](#usage-)
  - [Visual Diff Viewer](#visual-diff-viewer)
- [AI Features](#ai-features-)
  - [Configuration](#configuration)
  - [Generate Commit Messages](#generate-commit-messages)
  - [Generate Git Commands](#generate-git-commands)
  - [Explain Changes](#explain-changes)
  - [Tips & Tricks](#tips--tricks)
  - [AI Providers](#ai-providers)
- [Coding Agent Integrations](#coding-agent-integrations-)
- [Advanced Configuration](#advanced-configuration-)
  - [Configuration File](#configuration-file)
  - [Configuration Precedence](#configuration-precedence)

## Getting Started 🔅

### Prerequisites
Before you begin, ensure you have:
1. `git` installed on your system
2. [fzf](https://github.com/junegunn/fzf) (optional) - Required for `luminatti explain --list` command
3. [mdcat](https://github.com/swsnr/mdcat) (optional) - Required for pretty output formatting

### Installation

Build and install from this checkout with Rust and Cargo:

```bash
cargo install --path . --locked
```

For the native desktop workspace on macOS:

```bash
./dev.sh
```

## Local development and releases

| Command | Purpose |
| --- | --- |
| `./dev.sh [desktop options]` | Run the desktop app locally from this checkout. |
| `./build.sh` | Install the command line app locally with Cargo. |
| `./release.sh 2.33.0` | On a clean, up-to-date `main`, commit the new version, push `main`, then push the `v2.33.0` tag. |

Pushing any `vX.Y.Z` tag whose commit is on `origin/main` starts the release
workflow. The tag must match the versions in `Cargo.toml` and `Cargo.lock`.
GitHub Actions builds the binaries and macOS app, creates the GitHub release,
and publishes the crate. The `release` environment must allow `v*` tags and
have its publishing credentials configured.

All Rust dependencies come from crates.io and are pinned in `Cargo.lock`.
No sibling checkout or other local project is required. Icons are embedded in
the executable. GPUI's `font-kit` feature supplies text rendering, and
`runtime_shaders` supports building with Apple's Command Line Tools.
The optional Radar helper builds from `tools/radar-layout` in this repository;
its Go dependencies are downloaded by Go.

The executable is `luminatti`. Configuration uses `luminatti.config.json`,
`~/.config/luminatti/`, and `LUMINATTI_*` environment variables. Desktop state lives
under Luminatti's own app-data directory.

## Usage 🔅

### Native macOS review workspace

Luminatti includes an optional native desktop reviewer built with the same GPUI
rendering layer as Zed. It retains Luminatti's paired side-by-side diff algorithm,
but intentionally never edits source files. The left file navigator and right
annotation rail are resizable by dragging their dividers. Its shader is compiled
at application startup, so the macOS build only needs Command Line Tools—not a
full Xcode installation.

For TypeScript and TSX changes, the Review workspace adds syntax colouring,
Tree-sitter symbol analysis, collapsible change families with counts, and a
file outline. Expand a family to see compact `file: change` rows, newest first;
click a row to open its diff, or hover for the full path and details.
If a project provides `vtsls` or `typescript-language-server` in
`node_modules/.bin` (or on `PATH`), Luminatti also shows live LSP symbols and
reference counts. The outline continues to work from the local AST when no
language server is installed. Workspace preferences, filters, tabs, review
state, and Radar state persist per repository in Luminatti's local app-data
directory; Luminatti does not add metadata directories to the repository.
Use the gear icon at the top right to choose **Dark** or **Light**. Appearance
is saved in the local user profile and applies to every repository.

Working-copy lines support session-only review comments. Click or drag to select
lines, use Command-click (Ctrl-click on other platforms) for separate lines, or
Shift-click for a range. Right-click the selection and choose **Add Comment**.
Commented lines retain a yellow marker until cleared. The Comments button in the
Changes toolbar opens the **Review Comments** tab; its dropdown also offers
**Clear Comments**. The tab opens automatically when a comment is added while
it is closed. **Copy** includes
all file/line references and comments in a follow-up prompt for a coding agent.
Comments stay in memory, scoped to each repository, and disappear when Luminatti exits.

The **Radar** button beside Changes opens a separate, closable tab showing file
dependencies in the context of the current comparison. Changed JavaScript and
TypeScript files appear in compact cards containing their directory (or package
for root files), filename,
change marker, and function counters: **A** added, **M** changed, **D** deleted,
plus unchanged function context. Cards start collapsed. The control in each
card's top-right corner expands or collapses its functions independently;
expansion survives refreshes and recalculates the layout. Clicking a filename
opens its diff; clicking a changed function jumps to the appropriate old or new
line. Hovering reveals full paths, truncated names, and resolved call targets.

Radar traces callers/importers back to roots and retains the unchanged files
connecting them to changes. Unchanged linear chains are collapsed by default
into “⋯ N unchanged files”. Use **Expand context / Collapse context** (the same
icons as the diff) for all chains, or click an individual chain. Expanded context
is grey. Roots, declared package entry points, branch/join points, and changed
files remain visible. Branches without changes or function context are omitted.
Cards adapt their width to filenames and expanded functions, with larger type
and clearer change borders. Shared dependencies appear only once.
Cycles are bounded groups whose internal arrows can be expanded without hiding
changed files. Isolated changes are hidden and counted, except declared package
entry points, which can appear on their own.

Radar uses [D2's TALA layout](https://d2lang.com/blog/tala-is-open-source/)
when the local `luminatti-radar-layout` helper is installed beside the Luminatti binary.
The macOS app build includes it. For a development build, run
`bash scripts/build-radar-layout.sh`, then build/run Luminatti with `--features desktop`.
Building the helper requires Go 1.27 (a recent Go installation can download the
required toolchain automatically). No Go or D2 installation is needed at runtime.
An absolute `LUMINATTI_RADAR_LAYOUT` path can select a separately installed helper.

TALA arranges sized cards and routes orthogonal arrows on a background worker;
fixed seeds make identical inputs reproducible. Expanding a card can rearrange
the graph. It receives only numeric sizes and connectivity, and runs locally.
The native layout appears immediately and remains in use if the helper is absent,
fails, exceeds five seconds, or the graph exceeds 160 layout units / 600 edges.
In the native layout, wide levels and disconnected trees wrap into additional
rows, and dependencies stay below their parents. See
[the helper notice](tools/radar-layout/NOTICE.md) for D2 source and licensing.

Arrows between cards mean “imports / depends on”. Expanded function lists also
include transitive unchanged callers and callees of changed functions. Local
callees sit below their callers with an indented **↳** marker; hover a function
to see its direct call targets and their files. Static local calls, named/default
imports, namespace imports, and named/star re-exports are resolved from each
snapshot. Dynamic dispatch, computed targets, and ambiguous or shadowed imports
are not guessed. Filters remove matching files and their call context as well
as import edges.

Stable arrows are muted;
added imports are green and removed imports are dashed red. The view overlays
before and after relationships, so a displayed cycle may span the comparison.
Ancestor paths are traced separately in each snapshot to avoid inventing
transitive paths by combining old-only and new-only edges.

Analysis runs in the background against the actual before/after snapshots,
including the working files rather than the index for working-tree reviews.
It resolves relative imports, re-exports, type-only and side-effect imports,
literal dynamic imports, local `tsconfig`/`jsconfig` path aliases and relative
config inheritance, and declared local package entry/exports paths. Named
functions, methods, arrow properties, and nested functions are qualified by
scope. A file changed outside a function still appears without function rows.
External packages, computed imports, CommonJS `require`, and paths that cannot
be resolved are counted in the footer. Other source languages are not analysed.
Files with read/parse failures are excluded from both snapshots and reported.

```bash
./dev.sh
./dev.sh main..feature --focus src/main.rs
```

For a double-clickable local app bundle:

```bash
bash scripts/build-macos-app.sh
open "target/Luminatti.app"
```

#### macOS preview distribution

Desktop preview builds are published with every GitHub release as separate
Apple Silicon (`Luminatti-macos-arm64.zip`) and Intel
(`Luminatti-macos-x64.zip`) app archives. Download the matching archive,
unzip it into `~/Applications`, then open **Luminatti**. On its first open,
macOS will warn that the app is from an unidentified developer; control-click
the app, choose **Open**, then confirm. This preview channel deliberately does
not require an Apple Developer account, signing certificate, or notarization.

The `luminatti` Homebrew cask is updated by the release workflow when the
`HOMEBREW_TAP_REPOSITORY` variable (`jteso/homebrew-tap`)
and `HOMEBREW_TAP_TOKEN` secret are configured in the repository's `release`
environment. The token needs permission to push to the tap. Install it with
`brew install --cask jteso/tap/luminatti`.

The app checks GitHub Releases in the background. When an update is available,
the green **Update _version_** button downloads the matching archive, verifies
its published SHA-256 checksum, replaces the user-owned app bundle after Luminatti
quits, and relaunches it. Keep the app in `~/Applications` rather than the
system `/Applications` folder so updates never need an administrator password.
The app asks you to choose a Git repository when opened from Finder; when
launched from a terminal inside a repository, it opens that repository directly.

`+ Note` creates an in-memory review note for the active file and current app
session. The native surface is kept separate from Luminatti's VCS and diff-model
modules in `src/desktop/` so the UI can evolve without coupling it to the
existing terminal renderer.

### Visual Diff Viewer

Launch an interactive side-by-side diff viewer in your terminal:
<img width="3456" height="2122" alt="image" src="https://github.com/user-attachments/assets/757e1187-1615-4e90-bee2-5a1f59ec0960" />

```bash
# View uncommitted changes
luminatti diff

# View changes for a specific commit
luminatti diff HEAD~1

# View changes between branches
luminatti diff main..feature/A

# View changes in a GitHub Pull Request
luminatti diff --pr 123 # (--pr is optional)
luminatti diff https://github.com/owner/repo/pull/123

# Open the PR associated with the current branch
luminatti diff --detect-pr

# Filter to specific files
luminatti diff --file src/main.rs --file src/lib.rs

# Watch mode - auto-refresh on file changes
luminatti diff --watch

# Stacked mode - review commits one by one
luminatti diff main..feature --stacked

# Jump to a specific file on open
luminatti diff --focus src/main.rs

# Soft-wrap long lines (also settable in luminatti.config.json)
luminatti diff --wrap
```

#### Stacked Diff Mode

Review a range of commits one at a time with `--stacked`:

```bash
luminatti diff main..feature --stacked
luminatti diff HEAD~5..HEAD --stacked
```

This displays each commit individually, letting you navigate through them:
- `ctrl+h` / `ctrl+l`: Previous / next commit
- Click the `‹` / `›` arrows in the header

The header shows the current commit position, SHA, and message. Viewed files are tracked per commit, so your progress is preserved when navigating.

When viewing a PR, you can mark files as viewed (syncs with GitHub) using the `space` keybinding.

#### Theme Configuration

Customize the diff viewer colors with preset themes:

```bash
# Using CLI flag
luminatti diff --theme dracula

# Using environment variable
LUMINATTI_THEME=catppuccin-mocha luminatti diff

# Or set permanently in config file (~/.config/luminatti/luminatti.config.json)
{
  "theme": "dracula"
}
```

**Available themes:**
| Theme | Value |
|-------|-------|
| Default (auto-detect) | `dark`, `light` |
| Catppuccin | `catppuccin-mocha`, `catppuccin-latte` |
| Dracula | `dracula` |
| Nord | `nord` |
| One Dark | `one-dark` |
| Gruvbox | `gruvbox-dark`, `gruvbox-light` |
| Solarized | `solarized-dark`, `solarized-light` |
| Flexoki | `flexoki-dark`, `flexoki-light` |

Priority: CLI flag > config file > `LUMINATTI_THEME` env var > OS auto-detect.

#### Selection & Annotations

**Selection**: Click-drag in the content area for character-level selection, or on line numbers for line-level selection. Selected text can be copied or annotated.

**Annotations**: Add review comments at three levels of granularity:
- **Selection** — select lines with mouse, press `i` to annotate the selected range
- **Hunk** — focus a hunk with `{`/`}`, press `i` to annotate the hunk
- **File** — press `i` with no selection or hunk focus to annotate the whole file

Annotated lines display a `▍` gutter indicator. Use `I` to view, edit, delete, copy, or export all annotations.

#### Keybindings

- `j/k` or arrow keys: Navigate
- `{/}`: Jump between hunks
- `w`: Toggle watch mode
- `tab`: Toggle sidebar
- `space`: Mark file as viewed
- `e`: Open file in editor
- `y`: Copy selection (or filename)
- `i`: Annotate selection / hunk / file
- `I`: View all annotations
- `ctrl+h/l`: Previous/next commit (stacked mode)
- `?`: Show all keybindings

## AI Features 🔅

Luminatti also bundles optional AI helpers for commit messages, explanations, and natural-language git commands. These require configuring an AI provider — the diff viewer above does not.

### Configuration

Run `luminatti configure` for interactive setup (provider, API key, model). Settings are saved to `~/.config/luminatti/luminatti.config.json`.

### Generate Commit Messages

Create meaningful commit messages for your staged changes:

```bash
# Basic usage - generates a commit message based on staged changes
luminatti draft
# Output: "feat(button.tsx): Update button color to blue"

# Add context for more meaningful messages
luminatti draft --context "match brand guidelines"
# Output: "feat(button.tsx): Update button color to align with brand identity guidelines"
```

### Generate Git Commands

Ask Luminatti to generate Git commands based on a natural language query:

```bash
luminatti operate "squash the last 3 commits into 1 with the message 'squashed commit'"
# Output: git reset --soft HEAD~3 && git commit -m "squashed commit" [y/N]
```

The command will display an explanation of what the generated command does, show any warnings for potentially dangerous operations, and prompt for confirmation before execution.

### Explain Changes

Understand what changed and why:

```bash
# Working directory or staged changes
luminatti explain
luminatti explain --staged

# Specific commits or ranges
luminatti explain HEAD
luminatti explain HEAD~3..HEAD
luminatti explain main..feature/A

# Ask specific questions
luminatti explain --query "What's the performance impact of these changes?"

# Interactive commit selection (requires: fzf)
luminatti explain --list
```

### Tips & Tricks

```bash
# Copy commit message to clipboard (macOS / Linux)
luminatti draft | pbcopy
luminatti draft | xclip -selection c

# Directly commit using the generated message
luminatti draft | git commit -F -
```

[lazygit](https://github.com/jesseduffield/lazygit) integration is available — see the [user config docs](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md) for binding `luminatti draft` to a custom command.

### AI Providers

Configure your preferred AI provider:

```bash
# Using CLI arguments
luminatti -p openai -k "your-api-key" -m "gpt-5-mini" draft

# Using environment variables
export LUMINATTI_AI_PROVIDER="openai"
export LUMINATTI_API_KEY="your-api-key"
export LUMINATTI_AI_MODEL="gpt-5-mini"
```

#### Supported Providers

| Provider | API Key Required | Models |
|----------|-----------------|---------|
| [OpenAI](https://platform.openai.com/docs/models) `openai` (Default) | Yes | `gpt-5.2`, `gpt-5`, `gpt-5-mini`, `gpt-5-nano`, `gpt-4.1`, `gpt-4.1-mini`, `o4-mini` (default: `gpt-5-mini`) |
| [Claude](https://www.anthropic.com/pricing) `claude` | Yes | `claude-sonnet-4-5-20250930`, `claude-opus-4-5-20251115`, `claude-haiku-4-5-20251015` (default: `claude-sonnet-4-5-20250930`) |
| [Gemini](https://ai.google.dev/) `gemini` | Yes (free tier) | `gemini-3-pro`, `gemini-3-flash-preview`, `gemini-2.5-pro`, `gemini-2.5-flash`, `gemini-2.5-flash-lite` (default: `gemini-2.5-flash`) |
| [Groq](https://console.groq.com/docs/models) `groq` | Yes (free) | `llama-3.3-70b-versatile`, `llama-3.1-8b-instant`, `meta-llama/llama-4-maverick-17b-128e-instruct`, `openai/gpt-oss-120b` (default: `llama-3.3-70b-versatile`) |
| [DeepSeek](https://www.deepseek.com/) `deepseek` | Yes | `deepseek-chat` (V3.2), `deepseek-reasoner` (default: `deepseek-chat`) |
| [xAI](https://x.ai/) `xai` | Yes | `grok-4`, `grok-4-mini`, `grok-4-mini-fast` (default: `grok-4-mini-fast`) |
| [OpenCode Zen](https://opencode.ai/docs/zen) `opencode-zen` | Yes | [see list](https://opencode.ai/docs/zen#models) (default: `claude-sonnet-4-5`) |
| [Ollama](https://github.com/ollama/ollama) `ollama` | No (local) | [see list](https://ollama.com/library) (default: `llama3.2`) |
| [OpenRouter](https://openrouter.ai/) `openrouter` | Yes | [see list](https://openrouter.ai/models) (default: `anthropic/claude-sonnet-4.5`) |
| [Vercel AI Gateway](https://vercel.com/docs/ai-gateway) `vercel` | Yes | [see list](https://vercel.com/docs/ai-gateway/supported-models) (default: `anthropic/claude-sonnet-4.5`) |

## Coding Agent Integrations 🔅

Use luminatti as the review surface for your coding agent. When the agent finishes a turn, shell-escape to luminatti, annotate the diff inline, and press `s` to send your annotations back as the agent's next prompt.

```
agent finishes turn → !luminatti diff → annotate → press `s`
→ stdout returns to the agent → agent fixes your notes
```

The mechanics are just stdin/stdout — no plugins, no extensions:

- `s` in the diff TUI opens a confirmation modal. On `Enter`, luminatti exits and writes the formatted annotations to stdout (the same text `y` copies to your clipboard).
- The TUI auto-routes to `/dev/tty` when stdout is captured, so the agent receives clean text — no escape codes.

Works with anything that has a shell-escape:

| Agent | How to trigger |
|-------|----------------|
| Claude Code | `!luminatti diff` |
| Codex | `!luminatti diff` |
| Any agent with shell access | `luminatti diff` from a tool/bash call |

Annotate with `i` (selection / hunk / file), press `s` → `Enter` to send. Press `q` to dismiss without sending.

## Advanced Configuration 🔅

### Configuration File
Luminatti supports configuration through a JSON file. You can place the configuration file in one of the following locations:

1. Project Root: Create a luminatti.config.json file in your project's root directory.
2. Custom Path: Specify a custom path using the --config CLI option.
3. Global Configuration (Optional): Place a luminatti.config.json file in your system's default configuration directory:
    - Linux/macOS: `~/.config/luminatti/luminatti.config.json`
    - Windows: `%USERPROFILE%\.config\luminatti\luminatti.config.json`

Luminatti will load configurations in the following order of priority:

1. CLI arguments (highest priority)
2. Configuration file specified by --config
3. Project root luminatti.config.json
4. Global configuration file (lowest priority)

```json
{
  "provider": "openai",
  "model": "gpt-5-mini",
  "api_key": "sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
  "theme": "catppuccin-mocha",
  "wrap": true,
  "draft": {
    "commit_types": {
      "docs": "Documentation only changes",
      "style": "Changes that do not affect the meaning of the code",
      "refactor": "A code change that neither fixes a bug nor adds a feature",
      "perf": "A code change that improves performance",
      "test": "Adding missing tests or correcting existing tests",
      "build": "Changes that affect the build system or external dependencies",
      "ci": "Changes to our CI configuration files and scripts",
      "chore": "Other changes that don't modify src or test files",
      "revert": "Reverts a previous commit",
      "feat": "A new feature",
      "fix": "A bug fix"
    }
  }
}
```

### Configuration Precedence
Options are applied in the following order (highest to lowest priority):
1. CLI Flags
2. Configuration File
3. Environment Variables
4. Default options

Example: Using different providers for different projects:
```bash
# Set global defaults in .zshrc/.bashrc
export LUMINATTI_AI_PROVIDER="openai"
export LUMINATTI_AI_MODEL="gpt-5-mini"
export LUMINATTI_API_KEY="sk-xxxxxxxxxxxxxxxxxxxxxxxx"

# Override per project using config file
{
  "provider": "ollama",
  "model": "llama3.2"
}

# Or override using CLI flags
luminatti -p "ollama" -m "llama3.2" draft
```
## Contributors

<a href="https://github.com/jteso/luminatti/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=jteso/luminatti" />
</a>

Made with [contrib.rocks](https://contrib.rocks).

### Interested in Contributing?

Contributions are welcome! Please feel free to submit a Pull Request.

# Star History

<p align="center">
  <a target="_blank" href="https://star-history.com/#jteso/luminatti&Date">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=jteso/luminatti&type=Date&theme=dark">
      <img alt="GitHub Star History for jteso/luminatti" src="https://api.star-history.com/svg?repos=jteso/luminatti&type=Date">
    </picture>
  </a>
</p>
