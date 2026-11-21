# SteamPipe templates

These files are not an upload. They contain no Steam login and no password.
`YOUR_APP_ID`, `YOUR_LINUX_DEPOT_ID`, `YOUR_WINDOWS_DEPOT_ID`, and
`YOUR_MACOS_DEPOT_ID` are placeholders. Replace them before anyone runs
steamcmd. `build.sh` refuses to print the steamcmd line until those
placeholders are gone, and it still does not run steamcmd.

The content root is `dist/`. Each depot takes one folder:
`penguin-pass-linux-x86_64`, `penguin-pass-windows-x86_64`, and
`penguin-pass-macos`. The steps around these files are in `docs/steam-release.md`.
