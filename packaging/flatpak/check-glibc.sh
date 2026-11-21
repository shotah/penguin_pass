#!/bin/sh
# check-glibc.sh MAX FILE [FILE...]
# Fail if any FILE needs a GLIBC symbol newer than MAX (for example 2.44).
# Prints the highest GLIBC symbol and the NEEDED list for the build log.
set -eu

if [ "$#" -lt 2 ]; then
    echo "usage: check-glibc.sh MAX FILE [FILE...]" >&2
    exit 2
fi

max=$1
shift
fail=0

for file in "$@"; do
    if [ ! -f "$file" ]; then
        echo "glibc: missing $file" >&2
        exit 1
    fi
    echo "glibc: $file"
    needed=$(objdump -p "$file") || exit 1
    printf '%s\n' "$needed" | awk '/NEEDED/{print}'
    symbols=$(objdump -T "$file") || exit 1
    tags=$(printf '%s\n' "$symbols" | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)*' | sed 's/^GLIBC_//' | sort -Vu || true)
    if [ -z "$tags" ]; then
        echo "glibc: $file has no GLIBC version tags"
        continue
    fi
    highest=$(printf '%s\n' "$tags" | tail -n 1)
    echo "glibc: $file highest GLIBC_$highest (runtime provides $max)"
    if ! printf '%s\n%s\n' "$highest" "$max" | sort -C -V; then
        echo "glibc: $file needs GLIBC_$highest, which is newer than $max" >&2
        fail=1
    fi
done

exit "$fail"
