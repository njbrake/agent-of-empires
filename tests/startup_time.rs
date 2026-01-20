//! Startup time regression test
//!
//! Guards against slow startup by measuring wall-clock time for initialization.

use std::process::Command;
use std::time::Instant;

#[test]
fn startup_check_completes_within_threshold() {
    let binary = env!("CARGO_BIN_EXE_aoe");

    let start = Instant::now();
    let status = Command::new(binary)
        .arg("--check")
        .status()
        .expect("Failed to run aoe --check");
    let elapsed = start.elapsed();

    assert!(
        status.success(),
        "aoe --check failed - tools may not be available in test environment"
    );

    assert!(
        elapsed.as_secs() < 2,
        "Startup took {:?}, exceeding 2 second threshold. \
         Check that tool detection uses lightweight methods (e.g., `which`) \
         instead of running full binaries.",
        elapsed
    );
}
