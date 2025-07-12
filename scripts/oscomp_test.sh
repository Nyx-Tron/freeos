#!/bin/bash

TIMEOUT=120s
EXIT_STATUS=0
ROOT=$(realpath $(dirname $0))/../
AX_ROOT=$ROOT/.arceos
S_PASS=0
S_FAILED=1
S_TIMEOUT=2
S_BUILD_FAILED=3

RED_C="\x1b[31;1m"
GREEN_C="\x1b[32;1m"
YELLOW_C="\x1b[33;1m"
CYAN_C="\x1b[36;1m"
BLOD_C="\x1b[1m"
END_C="\x1b[0m"

if [ -z "$ARCH" ]; then
    ARCH=x86_64
fi
if [ "$ARCH" != "x86_64" ] && [ "$ARCH" != "riscv64" ] && [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "loongarch64" ]; then
    echo "Unknown architecture: $ARCH"
    exit $S_FAILED
fi

LIBC=musl

if [ "$LIBC" != "musl" ] && [ "$LIBC" != "glibc" ]; then
    echo "Unknown libc: $LIBC"
    exit $S_FAILED
fi

# TODO: add more basic testcases
basic_testlist=(
    "/$LIBC/basic/brk"
    "/$LIBC/basic/chdir"
    "/$LIBC/basic/clone"
    "/$LIBC/basic/close"
    "/$LIBC/basic/dup2"
    "/$LIBC/basic/dup"
    "/$LIBC/basic/execve"
    "/$LIBC/basic/exit"
    "/$LIBC/basic/fork"
    "/$LIBC/basic/fstat"
    "/$LIBC/basic/getcwd"
    "/$LIBC/basic/getdents"
    "/$LIBC/basic/getpid"
    "/$LIBC/basic/getppid"
    "/$LIBC/basic/gettimeofday"
    "/$LIBC/basic/mkdir_"
    "/$LIBC/basic/mmap"
    "/$LIBC/basic/mount"
    "/$LIBC/basic/munmap"
    "/$LIBC/basic/openat"
    "/$LIBC/basic/open"
    "/$LIBC/basic/pipe"
    "/$LIBC/basic/read"
    "/$LIBC/basic/times"
    "/$LIBC/basic/umount"
    "/$LIBC/basic/uname"
    "/$LIBC/basic/unlink"
    "/$LIBC/basic/wait"
    "/$LIBC/basic/waitpid"
    "/$LIBC/basic/write"
    "/$LIBC/basic/yield"
)
busybox_testlist=(
    "/$LIBC/busybox ash -c exit"
    "/$LIBC/busybox sh -c exit"
    "/$LIBC/busybox basename /aaa/bbb"
    "/$LIBC/busybox cal"
    "/$LIBC/busybox clear"
    "/$LIBC/busybox date"
    "/$LIBC/busybox df"
    "/$LIBC/busybox dirname /aaa/bbb"
    "/$LIBC/busybox dmesg"
    "/$LIBC/busybox du /musl/busybox"
    "/$LIBC/busybox expr 1 + 1"
    "/$LIBC/busybox false"
    "/$LIBC/busybox true"
    "/$LIBC/busybox which ls"
    "/$LIBC/busybox uname"
    "/$LIBC/busybox uptime"
    "/$LIBC/busybox printf \"abc\n\""
    "/$LIBC/busybox ps"
    "/$LIBC/busybox pwd"
    "/$LIBC/busybox free"
    "/$LIBC/busybox hwclock"
    "/$LIBC/busybox sh -c 'sleep 5' & /$LIBC/busybox kill \$!"
    "/$LIBC/busybox ls"
    "/$LIBC/busybox sleep 1"
    "/$LIBC/busybox echo \"#### file opration test\""
    "/$LIBC/busybox touch test.txt"
    "/$LIBC/busybox sh -c 'echo \"hello world\" > test.txt'"
    "/$LIBC/busybox cat test.txt"
    "/$LIBC/busybox cut -c 3 test.txt"
    "/$LIBC/busybox od test.txt"
    "/$LIBC/busybox head test.txt"
    "/$LIBC/busybox tail test.txt"
    "/$LIBC/busybox hexdump -C test.txt"
    "/$LIBC/busybox md5sum test.txt"
    "/$LIBC/busybox sh -c 'echo \"ccccccc\" >> test.txt'"
    "/$LIBC/busybox sh -c 'echo \"bbbbbbb\" >> test.txt'"
    "/$LIBC/busybox sh -c 'echo \"aaaaaaa\" >> test.txt'"
    "/$LIBC/busybox sh -c 'echo \"2222222\" >> test.txt'"
    "/$LIBC/busybox sh -c 'echo \"1111111\" >> test.txt'"
    "/$LIBC/busybox sh -c 'echo \"bbbbbbb\" >> test.txt'"
    "/$LIBC/busybox sh -c 'sort test.txt | /$LIBC/busybox uniq'"
    "/$LIBC/busybox stat test.txt"
    "/$LIBC/busybox strings test.txt"
    "/$LIBC/busybox wc test.txt"
    "/$LIBC/busybox sh -c '[ -f test.txt ]'"
    "/$LIBC/busybox more test.txt"
    "/$LIBC/busybox rm test.txt"
    "/$LIBC/busybox mkdir test_dir"
    "/$LIBC/busybox mv test_dir test"
    "/$LIBC/busybox rmdir test"
    "/$LIBC/busybox grep hello busybox_cmd.txt"
    "/$LIBC/busybox cp busybox_cmd.txt busybox_cmd.bak"
    "/$LIBC/busybox rm busybox_cmd.bak"
    "/$LIBC/busybox find -maxdepth 1 -name \"busybox_cmd.txt\""
)
iozone_testlist=("/$LIBC/busybox sh /$LIBC/iozone_testcode.sh")
lua_testlist=("/$LIBC/busybox sh /$LIBC/lua_testcode.sh")
libctest_testlist=()

testcases_type=(
    "basic"
    "busybox"
    "lua"
    "libctest"
)

IMG_URL=https://github.com/Azure-stars/testsuits-for-oskernel/releases/download/v0.2/sdcard-$ARCH.img.gz
if [ ! -f sdcard-$ARCH.img ]; then
    echo -e "${CYAN_C}Downloading${END_C} $IMG_URL"
    wget -q $IMG_URL
    gunzip sdcard-$ARCH.img.gz
    if [ $? -ne 0 ]; then
        echo -e "${RED_C}download failed!${END_C}"
        exit 1
    fi
fi

cp sdcard-$ARCH.img $AX_ROOT/disk.img

ARG="AX_TESTCASE=oscomp ARCH=$ARCH EXTRA_CONFIG=../configs/$ARCH.toml BLK=y NET=y FEATURES=fp_simd,lwext4_rs SMP=4 ACCEL=n LOG=off"

echo -e "${GREEN_C}ARGS:${END_C} $ARG"
if [ $? -ne 0 ]; then
    echo -e "${RED_C}build failed!${END_C}"
fi

function test_one() {
    local testcase_type=$1
    local actual="apps/oscomp/actual_$testcase_type.out"
    RUN_TIME=$( { time { timeout --foreground $TIMEOUT make -C "$ROOT" $ARG run > "$actual" ; }; } )
    local res=$?
    if [ $res == 124 ]; then
        res=$S_TIMEOUT
    elif [ $res -ne 0 ]; then
        res=$S_FAILED
    else 
        res=$S_PASS
    fi
    cat "$actual"
    if [ $res -ne $S_PASS ]; then
        EXIT_STATUS=$res
        if [ $res == $S_FAILED ]; then
            echo -e "${RED_C}failed!${END_C} $RUN_TIME"
        elif [ $res == $S_TIMEOUT ]; then
            echo -e "${YELLOW_C}timeout!${END_C} $RUN_TIME"
        elif [ $res == $S_BUILD_FAILED ]; then
            echo -e "${RED_C}build failed!${END_C}"
        fi
        echo -e "${RED_C}actual output${END_C}:"
    else
        local judge_script="${ROOT}apps/oscomp/judge_${testcase_type}.py"
        python3 $judge_script < "$actual"
        if [ $? -ne 0 ]; then
            echo -e "${RED_C}failed!${END_C}"
            EXIT_STATUS=$S_FAILED
        else
            echo -e "${GREEN_C}passed!${END_C} $RUN_TIME"
            rm -f "$actual"
        fi
    fi
}

for type in "${testcases_type[@]}"; do
    declare -n test_list="${type}_testlist"
    echo -e "${CYAN_C}Testing $type testcases${END_C}"

    # clean the testcase_list file
    rm -f $ROOT/apps/oscomp/testcase_list
    for t in "${test_list[@]}"; do
        echo $t >> $ROOT/apps/oscomp/testcase_list
    done
    test_one "$type"
done

echo -e "test script exited with: $EXIT_STATUS"
exit $EXIT_STATUS
