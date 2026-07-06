//! Build script for the `ayx` binary.
//!
//! Reserve a larger main-thread stack on Windows.
//!
//! The MSVC toolchain defaults the main-thread stack to 1 MiB. `ayx` has a deep
//! clap command tree (dozens of nested subcommands); in debug/test builds,
//! constructing that `Command` tree during `Cli::parse()` recurses deeply
//! enough to overflow 1 MiB and abort with `STATUS_STACK_OVERFLOW`
//! (`0xC00000FD`). This reproduced on every invocation on `windows-latest` CI
//! runners — even `ayx --version` — while Linux/macOS (8 MiB default main-thread
//! stack) and release Windows installs were unaffected. See issue #59 Part 2.
//!
//! `Cli::parse()` runs on the process main thread, before `main()` moves command
//! dispatch onto a 16 MiB worker thread, so that worker-thread stack never
//! covered the parse path. Reserving 16 MiB for the main thread at link time
//! fixes the actual constraint.
//!
//! `rustc-link-arg-bins` scopes the flag to binary targets and — unlike
//! `.cargo/config.toml` `rustflags` — is not shadowed when CI sets the
//! `RUSTFLAGS` environment variable.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV");
    if target_os.as_deref() == Ok("windows") && target_env.as_deref() == Ok("msvc") {
        // 16 MiB reserve, matching the command-dispatch worker-thread stack.
        println!("cargo:rustc-link-arg-bins=/STACK:16777216");
    }
}
