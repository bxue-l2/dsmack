// SPDX-License-Identifier: Apache-2.0
// Minimal ARM64 Image. CPU 0 starts the other CPUs with PSCI and waits for
// their shared-memory acknowledgement before powering off. MODE selects the
// state the other CPUs occupy when shutdown arrives.
.text
.p2align 3
.globl _start
_start:
image_start:
    b primary
    .long 0
    .quad 0
    .quad image_end - image_start
    .quad 0
    .quad 0
    .quad 0
    .quad 0
    .ascii "ARM\x64"
    .long 0

primary:
    adr x20, seen
#if MODE != 0
    mov x21, #1
start_next:
    cmp x21, #CPU_COUNT
    b.ge wait_all
    movz x0, #3
    movk x0, #0xc400, lsl #16 // PSCI CPU_ON
    mov x1, x21
    adr x2, secondary
    mov x3, #0
    hvc #0
    add x21, x21, #1
    b start_next
wait_all:
    mov x21, #1
wait_next:
    cmp x21, #CPU_COUNT
    b.ge ready
    ldr x0, [x20, x21, lsl #3]
    cbz x0, wait_next
    add x21, x21, #1
    b wait_next
#endif
ready:
#if MODE == 3 || MODE == 4 || MODE == 5
    b ready
#else
#if MODE == 2
    // Leave time for the secondary CPU to enter WFI before requesting shutdown.
    mrs x1, cntfrq_el0
    mov x2, #100
    udiv x1, x1, x2
    mrs x2, cntvct_el0
    add x1, x1, x2
wait_idle:
    mrs x2, cntvct_el0
    cmp x2, x1
    b.lo wait_idle
#endif
    b power_off
#endif

secondary:
    mrs x21, mpidr_el1
    and x21, x21, #0xff
    adr x20, seen
    mov x0, #1
    str x0, [x20, x21, lsl #3]
    dsb sy
#if MODE == 3
    b power_off
#elif MODE == 4
    mov x0, #-1 // Unsupported PSCI call: the vCPU thread must fail closed.
    hvc #0
#endif
secondary_loop:
#if MODE == 2
    wfi
#else
    add x0, x0, #1
    str x0, [x20, x21, lsl #3]
#endif
    b secondary_loop

power_off:
    movz x0, #8
    movk x0, #0x8400, lsl #16 // PSCI SYSTEM_OFF
    hvc #0
    b power_off

.p2align 3
seen:
    .space CPU_COUNT * 8
image_end:
