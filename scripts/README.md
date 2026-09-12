# Scripts

Cross-platform maintenance scripts live here when a workflow cannot be expressed as a root pnpm or Cargo command.

## `build-portable.ps1`

Builds the self-contained Windows executable.

```powershell
pnpm build:portable                                  # -> release/Dev Workbench.exe
powershell -File scripts/build-portable.ps1 -OutDir D:\apps
powershell -File scripts/build-portable.ps1 -Configuration debug
```

| Parameter | Default | Purpose |
|---|---|---|
| `-OutDir` | `<repo>/release` | Directory that receives the executable |
| `-Configuration` | `release` | Cargo profile to build; `debug` is available for a faster, larger, unoptimised build |

The script builds the frontend, runs `tauri build --no-bundle`, and copies the binary to `release/Dev Workbench.exe`. The output name is deliberately version-free so a pinned shortcut survives a rebuild; the version is printed instead of encoded.

Requirements: Windows, and Windows PowerShell 5.1 or later. The script is written against 5.1 so it does not depend on PowerShell 7 being installed. Application data is not affected — it stays in the platform application data directory, so moving the executable does not move the database.
