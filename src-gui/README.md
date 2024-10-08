## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Requirements

- For compiling the Rust code: `cargo` and `cargo tauri` ([installation](https://v2.tauri.app/reference/cli/))
- For running the Typescript code: `node` and `yarn`
- For formatting and bindings: `dprint` (`cargo install dprint@0.39.1`) and `typeshare` (`cargo install typeshare-cli`)
- If you are on Windows and you want to use the `check-bindings` command you'll need to manually install the GNU DiffUtils ([installation](https://gnuwin32.sourceforge.net/packages/diffutils.htm)) and GNU CoreUtils ([installtion](https://gnuwin32.sourceforge.net/packages/coreutils.htm)). Remember to add the installation path (probably `C:\Program Files (x86)\GnuWin32\bin`) to the `PATH` in your enviroment variables.

## Start development servers

For development, we need to run both `vite` and `tauri` servers:

```bash
cd src-gui
yarn install && yarn run dev
# let this run
```

```bash
cd src-tauri
cargo tauri dev
# let this run as well
```

## Generate bindings for Tauri API

Running `yarn run dev` or `yarn build` should automatically re-build the Typescript bindings whenever something changes. You can also manually trigger this using the `gen-bindings` command:

```bash
yarn run gen-bindings
```

You can also check whether the current bindings are up to date:

```bash
yarn run check-bindings
```
