#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::fs::{self, File};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use krun::api::{Payload, VerifiedBootPins, VmmBuilder};
use sha2::{Digest, Sha256};

fn run_case(case: &str, cpus: u8, aborts: bool) {
    // An HVF VM is process-wide. A subprocess also lets us assert that a failed
    // execution terminates without returning to the code which finalizes results.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "guest_process", "--ignored", "--nocapture"])
        .env("DSMACK_TEST_CASE", case)
        .env("DSMACK_TEST_CPUS", cpus.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("{case}: hung\n{}", String::from_utf8_lossy(&output.stderr));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if aborts {
        assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{stderr}");
        assert!(!stdout.contains("VM_RETURNED"), "{stdout}");
        let reason = match case {
            "error" => "Unexpected val=",
            "timeout" => "execution budget exceeded",
            "cancel" | "teardown-timeout" => "native teardown failed:",
            _ => unreachable!(),
        };
        assert!(stderr.contains(reason), "{case}: {stderr}");
    } else {
        assert!(output.status.success(), "{case}: {stdout}\n{stderr}");
        assert_eq!(stdout.matches("VM_RETURNED").count(), 5, "{stdout}");
    }
}

#[test]
#[ignore = "run through runner-vm/native/test-multicpu.sh for HVF signing"]
fn single_cpu_still_returns() {
    run_case("unstarted", 1, false);
}

#[test]
#[ignore = "requires HVF signing"]
fn stops_secondary_cpu_before_cpu_on() {
    run_case("unstarted", 2, false);
}

#[test]
#[ignore = "requires HVF signing"]
fn stops_busy_secondary_cpu() {
    run_case("busy", 2, false);
}

#[test]
#[ignore = "requires HVF signing"]
fn stops_idle_secondary_cpu() {
    run_case("idle", 2, false);
}

#[test]
#[ignore = "requires HVF signing"]
fn secondary_cpu_can_request_shutdown() {
    run_case("secondary-halt", 2, false);
}

#[test]
#[ignore = "requires HVF signing"]
fn stops_three_busy_secondary_cpus() {
    run_case("busy", 4, false);
}

#[test]
#[ignore = "requires HVF signing"]
fn secondary_cpu_failure_produces_no_result() {
    run_case("error", 2, true);
}

#[test]
#[ignore = "requires HVF signing"]
fn multicpu_execution_timeout_produces_no_result() {
    run_case("timeout", 2, true);
}

#[test]
#[ignore = "requires HVF signing"]
fn multicpu_cancel_produces_no_result() {
    run_case("cancel", 2, true);
}

#[test]
#[ignore = "requires HVF signing"]
fn multicpu_teardown_timeout_produces_no_result() {
    run_case("teardown-timeout", 2, true);
}

#[test]
#[ignore = "requires an ARM64 Linux Image and static busybox; see test-multicpu.sh"]
fn linux_multicpu_boot() {
    run_case("linux", 2, false);
}

#[test]
#[ignore = "subprocess entry point"]
fn guest_process() {
    let case = std::env::var("DSMACK_TEST_CASE").unwrap();
    let cpus: u8 = std::env::var("DSMACK_TEST_CPUS").unwrap().parse().unwrap();
    let dir = PathBuf::from(std::env::var_os("DSMACK_TEST_GUEST_DIR").unwrap());
    let kernel_path = if case == "linux" {
        PathBuf::from(std::env::var_os("DSMACK_TEST_LINUX_KERNEL").unwrap())
    } else {
        dir.join(format!("{case}-{cpus}.bin"))
    };
    let kernel = fs::read(kernel_path).unwrap();
    let initrd = (case == "linux").then(|| fs::read(dir.join("linux-initrd")).unwrap());
    let cmdline = if case == "linux" {
        format!("console=hvc0 panic=-1 expected_cpus={cpus}")
    } else {
        String::new()
    };
    let kernel_hash = format!("{:x}", Sha256::digest(&kernel));
    let initrd_hash = initrd
        .as_ref()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    let cmdline_hash = format!("{:x}", Sha256::digest(cmdline.as_bytes()));
    let budget = if case == "timeout" {
        Duration::from_millis(250)
    } else {
        Duration::from_secs(5)
    };

    for iteration in 0..5 {
        let pins = VerifiedBootPins {
            kernel_sha256: &kernel_hash,
            initrd_sha256: initrd_hash.as_deref(),
            cmdline_sha256: &cmdline_hash,
        };
        let payload =
            Payload::load_external_raw_verified(&kernel, initrd.as_deref(), &cmdline, &pins)
                .unwrap();
        let console_path = dir.join(format!(
            "{case}-{cpus}-{}-{iteration}.console",
            std::process::id()
        ));
        let console = File::create(&console_path).unwrap();
        let vm = VmmBuilder::new()
            .vcpus(cpus)
            .unwrap()
            .ram_mib(if case == "linux" { 256 } else { 64 })
            .unwrap()
            .payload(payload)
            .returning_console(console)
            .exec_budget(budget)
            .build()
            .unwrap();
        let exit = vm.run_returning_with_cancel(
            Arc::new(AtomicBool::new(case == "cancel")),
            if case == "teardown-timeout" {
                Duration::ZERO
            } else {
                Duration::from_secs(5)
            },
        );
        assert_eq!(exit, 0);
        if case == "linux" {
            let log = fs::read_to_string(&console_path).unwrap();
            assert!(log.contains(&format!("LINUX_CPUS={cpus}")), "{log}");
            assert!(log.contains("LINUX_WORKLOAD_OK"), "{log}");
        }
        println!("VM_RETURNED");
    }
}
