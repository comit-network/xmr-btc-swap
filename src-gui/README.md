## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Requirements

- For compiling the Rust code: `cargo` and `cargo tauri` ([installation](https://v2.tauri.app/reference/cli/))
- For running the Typescript code: `node` and `yarn`
- For formatting and bindings: `dprint` (`cargo install dprint@0.50.0`) and `typeshare` (`cargo install typeshare-cli`)
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
cargo tauri dev --no-watch -- -- --testnet
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

## Debugging

Because the GUI is running in an embedded browser, we can't use the usual Browser extensions to debug the GUI. Instead we use standalone React DevTools / Redux DevTools.

### React DevTools

Run this command to start the React DevTools server. The frontend will connect to this server automatically:

```bash
npx react-devtools
```

### Redux DevTools

Run this command to start the Redux DevTools server. The frontend will connect to this server automatically. You can then debug the global Redux state. Observe how it changes over time, go back in time, see dispatch history, etc.

You may have to go to `Settings -> 'use local custom server' -> connect` inside the devtools window for the state to be reflected correctly.

```bash
npx redux-devtools --hostname=localhost --port=8098 --open
```
