# SteamPipe templates

These files are not an upload. They contain no Steam login and no password.
`YOUR_APP_ID` and `YOUR_LINUX_DEPOT_ID` are placeholders. Replace both before
anyone runs steamcmd. `build.sh` refuses to print the steamcmd line until
those placeholders are gone, and it still does not run steamcmd.

The content root is `dist/penguin-pass-linux-x86_64`, produced by `./x export`.
The steps around these files are in `docs/steam-release.md`.
