#!/bin/sh
# Exercise the returning lifecycle on Apple Silicon without a Linux image or App Attest.
set -eu
cd "$(dirname "$0")"
native="$(pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/dsmack-multicpu.XXXXXX")"
trap 'rm -rf "$work"' EXIT HUP INT TERM
export DSMACK_TEST_GUEST_DIR="$work"

set -- --skip linux_multicpu_boot
if [ -n "${DSMACK_TEST_LINUX_KERNEL:-}" ]; then
    : "${DSMACK_TEST_BUSYBOX:?Set DSMACK_TEST_BUSYBOX to a static ARM64 busybox}"
    mkdir -p "$work/root/bin" "$work/root/dev" "$work/root/proc" "$work/root/sys" "$work/root/tmp"
    cp "$DSMACK_TEST_BUSYBOX" "$work/root/bin/busybox"
    cp source-tree/apple-static/tests/guests/linux-init.sh "$work/root/init"
    chmod +x "$work/root/init" "$work/root/bin/busybox"
    tar --format=newc -cf "$work/linux-initrd" -C "$work/root" .
    set --
fi

for config in unstarted:0:1 unstarted:0:2 busy:1:2 idle:2:2 secondary-halt:3:2 \
              error:4:2 timeout:5:2 cancel:1:2 teardown-timeout:1:2 busy:1:4; do
    name="${config%%:*}"
    rest="${config#*:}"
    mode="${rest%%:*}"
    cpus="${rest#*:}"
    xcrun clang -arch arm64 -mmacosx-version-min=14.0 -c -DMODE="$mode" -DCPU_COUNT="$cpus" \
        source-tree/apple-static/tests/guests/multicpu.S -o "$work/guest.o"
    xcrun ld -arch arm64 -e _start -platform_version macos 14.0 14.0 \
        -syslibroot "$(xcrun --show-sdk-path)" -lSystem "$work/guest.o" -o "$work/guest"
    xcrun segedit "$work/guest" -extract __TEXT __text "$work/$name-$cpus.bin"
done

"$native/build.sh" --test returning_multicpu --message-format=json > "$work/build.jsonl"
binary="$(python3 - "$work/build.jsonl" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    message = json.loads(line)
    if (message.get("reason") == "compiler-artifact"
            and message["target"]["name"] == "returning_multicpu"
            and message.get("executable")):
        print(message["executable"])
PY
)"
test -n "$binary"
cat > "$work/hvf.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>com.apple.security.hypervisor</key><true/></dict></plist>
PLIST
codesign --force --sign - --entitlements "$work/hvf.plist" "$binary"
"$binary" --ignored --skip guest_process "$@" --test-threads=1 --nocapture
