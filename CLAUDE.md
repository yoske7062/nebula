# Nebula

Tauri 2 (Rust) + React 19 desktop app that downloads League of Legends skins and applies
them through `mod-tools` (from cslol-manager). Runs on macOS and Windows.

## Layout

- `src/` React UI. `src-tauri/src/commands/` Tauri commands (skins, patcher, game path, auto-apply, bridge).
- `src-tauri/src/macos.rs` macOS-only startup (relaunch as root). Everything else is shared;
  platform differences are `#[cfg(windows)]` / `#[cfg(not(windows))]` next to the code they affect.
- `mod-tools/` is the macOS patcher only. On Windows, upstream cslol-tools `mod-tools.exe`
  (plus `cslol-dll.dll`, which it needs to start) only runs `import` and `mkoverlay`; cslol's
  `runoverlay` no longer applies anything on current League builds.
- Windows injection goes through LTK Manager's `ltk_patcher_host.exe` + `ltk_patcher_dll.dll`
  (`src-tauri/src/commands/ltk.rs`), found in `%APPDATA%\dev.leaguetoolkit.manager\patcher\<digest>\`.
  Nebula does not ship them: the user installs LTK Manager and opens it once so it downloads
  the DLL. Patcher output goes to `%APPDATA%\com.nebula.app\patcher.log`.
- `client-plugin/` + `scripts/pengu-install.sh`: in-client skin picker via Pengu Loader, macOS only so far.

## Windows dev setup

Needs Node 20, Rust stable (MSVC toolchain), Visual Studio 2022 Build Tools with C++, CMake, Git.

Build the mod-tools sidecar once (PowerShell, repo root):

```powershell
git clone https://github.com/LeagueToolkit/cslol-manager.git cslol
git -C cslol checkout 23f230858bc2359ce279e07ed129d482fe3b00bf
cmake -S cslol/cslol-tools -B cslol/build -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded
cmake --build cslol/build --config Release --target mod-tools
New-Item -ItemType Directory -Force src-tauri/binaries
Copy-Item cslol/build/Release/mod-tools.exe src-tauri/binaries/mod-tools-x86_64-pc-windows-msvc.exe
Copy-Item cslol/cslol-tools/vendor/cslol-patcher/cslol-dll.dll src-tauri/binaries/
```

`src-tauri/binaries/` is gitignored; do not commit the binaries or the `cslol/` checkout.

Then:

```powershell
npm ci
npm run tauri dev      # run the app
npm run tauri build    # installer at src-tauri/target/release/bundle/nsis/
```

Rust tests: `cargo test` inside `src-tauri/`.

## Releases

Pushing to `windows-port` runs `.github/workflows/windows.yaml`, which builds the NSIS
installer and publishes it as a GitHub release. Put `[skip ci]` in the commit message for
changes that should not produce a release. `release.yaml` (macOS dmg) runs on pushes to `main`.
