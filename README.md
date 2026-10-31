<p align="center">
  <img src="icon.png" alt="Nebula" width="120" />
</p>

<h1 align="center">Nebula</h1>

<p align="center">
  <a href="https://github.com/yoske7062/nebula/releases/latest"><img src="https://img.shields.io/badge/download-latest-c89b3c?style=for-the-badge" alt="Download" /></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License" />
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-black" alt="Platform" />
  <img src="https://img.shields.io/badge/tauri-2-24C8D8" alt="Tauri" />
  <img src="https://img.shields.io/badge/react-19-61DAFB" alt="React" />
  <img src="https://img.shields.io/badge/version-0.1.12-c89b3c" alt="Version" />
</p>

<p align="center">
  A lightweight desktop app to browse and apply League of Legends skins.
</p>

---

<p align="center">
  <img src="preview.png" alt="Nebula Preview" width="700" />
</p>

## Features

- Browse all champions and their skins with splash art previews
- Download individual skins or batch download all skins for a champion
- Select and apply multiple skins at once - one per champion
- Import custom mods (.fantome/.zip)
- Real-time patcher status (importing, waiting for game, skins active, etc.)
- Game auto-detection - finds your League installation automatically
- Storage management - clear downloaded skins and patcher data
- macOS (Apple Silicon and Intel) and Windows

## Installation

### Windows

1. Install [LTK Manager](https://github.com/LeagueToolkit/ltk-manager/releases/latest), open it once so it downloads its patcher, then close it. Nebula injects skins through LTK's patcher and does not ship it.
2. Download `Nebula_x.x.x_x64-setup.exe` from the [Releases page](https://github.com/yoske7062/nebula/releases/latest) and run it.
3. The installer is unsigned, so SmartScreen may say **"Windows protected your PC"**: click **More info**, then **Run anyway**.
4. Nebula asks for administrator rights each time it starts: the patcher cannot reach the game without them.
5. If League is not in `C:\Riot Games\League of Legends`, pick its folder (or `LeagueClient.exe`) when Nebula asks.

If a skin does not show in game, the patcher's output is in `%APPDATA%\com.nebula.app\patcher.log`. When LTK reports its patcher is out of date, open LTK Manager so it updates.

### macOS

1. Download the latest `.dmg` for your Mac from the [Releases page](https://github.com/Mouadzz/nebula/releases/latest):
   - **Apple Silicon** (M1/M2/M3/M4/M5...) - `Nebula_x.x.x_aarch64.dmg`
   - **Intel** - `Nebula_x.x.x_x64.dmg`
2. Open the `.dmg` and drag Nebula to your Applications folder
3. On first launch, macOS may show **"Nebula is damaged and can't be opened"** - this is normal for ad-hoc signed apps. To fix this, run in Terminal:
   ```
   xattr -cr /Applications/Nebula.app
   ```
   Then open the app again normally.

## Usage

1. **Start League client** - Open the Riot/League client first and leave it running.
2. **Run Nebula** - Open Nebula once the client is up.
3. **Download & apply skins** - Pick a champion, download skins if needed, then go to **My Skins**, select one skin per champion, and click **Apply**.
4. **Start your game** - With the patcher showing **Waiting for game**, queue up and play.

> **Important:** Apply the skin before champion select ends (use the default skin in client) so it is locked in when the game starts. If it does not apply, close the game, reopen League, run Nebula again, apply, and reconnect.

## Disclaimer

Nebula is not endorsed by Riot Games and does not reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games and all associated properties are trademarks or registered trademarks of Riot Games, Inc.

Skins are visible only to you and do not affect gameplay or provide any competitive advantage. Use at your own risk.


## License

This project is licensed under the MIT License.

## Credits

- [cslol-manager](https://github.com/LeagueToolkit/cslol-manager) - modding tools
- [LTK Manager](https://github.com/LeagueToolkit/ltk-manager) - the patcher used on Windows
- [Rose](https://github.com/Alban1911/Rose) - idea of detecting champion select through the client API
- [LeagueSkins](https://github.com/Alban1911/LeagueSkins) - a comprehensive collection of League of Legends skin assets, organized by champion and skin IDs
