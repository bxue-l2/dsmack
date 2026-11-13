#!/bin/busybox sh
set -eu
export PATH=/bin
/bin/busybox --install -s /bin
mount -t devtmpfs devtmpfs /dev
exec > /dev/console 2>&1
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t tmpfs tmpfs /tmp

expected=0
for arg in $(cat /proc/cmdline); do
    case "$arg" in expected_cpus=*) expected="${arg#*=}" ;; esac
done
actual="$(grep -c '^processor' /proc/cpuinfo)"
echo "LINUX_CPUS=$actual"
test "$actual" -eq "$expected"

pids=""
cpu=0
while [ "$cpu" -lt "$expected" ]; do
    taskset -c "$cpu" sh -c '
        dd if=/dev/zero of="/tmp/cpu-$1" bs=1048576 count=4
        sha256sum "/tmp/cpu-$1"
    ' sh "$cpu" &
    pids="$pids $!"
    cpu=$((cpu + 1))
done
for pid in $pids; do wait "$pid"; done
sync
echo LINUX_WORKLOAD_OK
poweroff -f
