#!/usr/bin/env bash
# SteamPipe template. Does not log in and does not upload.
# Replace YOUR_APP_ID and YOUR_LINUX_DEPOT_ID, YOUR_WINDOWS_DEPOT_ID,
# and YOUR_MACOS_DEPOT_ID in the vdf files first.
# Then run steamcmd yourself with a Steamworks build account.
# https://partner.steamgames.com/doc/sdk/uploading

set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)

if grep -q 'YOUR_APP_ID\|YOUR_LINUX_DEPOT_ID\|YOUR_WINDOWS_DEPOT_ID\|YOUR_MACOS_DEPOT_ID' "$HERE"/*.vdf; then
    echo "Replace YOUR_APP_ID and the YOUR_*_DEPOT_ID placeholders in tools/steam/*.vdf first." >&2
    exit 1
fi

echo "This script does not upload. From a Steamworks SDK steamcmd checkout, run:"
echo "steamcmd +login YOUR_BUILD_ACCOUNT +run_app_build \"$HERE/app_build.vdf\" +quit"
echo "Set Preview to \"1\" in app_build.vdf for a local file list that is not uploaded."
