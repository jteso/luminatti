# Luminatti

Luminatti is a native macOS app for reviewing changes in Git and Jujutsu repositories. Open a repository to browse changed files, compare diffs side by side, and leave review comments.

The desktop workspace includes file filters, tabs, themes, and a Radar view that maps JavaScript and TypeScript imports and local function calls around changed files. Review comments stay in memory for the session and can be copied for a handoff.

## Install with Homebrew

On macOS 13 or later, install the app from the [Homebrew tap](https://github.com/jteso/homebrew-tap):

```sh
brew install --cask jteso/tap/luminatti
```

Homebrew opens **Luminatti** after installation. Choose a repository when prompted. You can reopen the app from Applications later.

The current macOS release is unsigned. If macOS blocks the first launch, follow [Apple's Open Anyway instructions](https://support.apple.com/en-au/102445).

## Install from source

Install [Rust](https://rustup.rs/), [Go](https://go.dev/doc/install), and the Xcode Command Line Tools, then build and install the macOS app:

```sh
git clone https://github.com/jteso/luminatti.git
cd luminatti
bash scripts/build-macos-app.sh
mkdir -p "$HOME/Applications"
ditto target/Luminatti.app "$HOME/Applications/Luminatti.app"
open "$HOME/Applications/Luminatti.app"
```

For live TypeScript symbols and reference counts, install `vtsls` or `typescript-language-server` in the repository or on your `PATH`. Radar's source analysis works without a language server.

## License

Luminatti is licensed under the [MIT License](LICENSE). The optional Radar layout helper includes D2 TALA components; see [`tools/radar-layout/NOTICE.md`](tools/radar-layout/NOTICE.md) and the accompanying license files.
