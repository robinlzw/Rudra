#![feature(box_patterns)]
#![feature(rustc_private)]
#![feature(try_blocks)]
#![feature(never_type)]

extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_hir_pretty;
extern crate rustc_index;
extern crate rustc_infer;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

#[macro_use]
extern crate bitflags;

#[macro_use]
extern crate if_chain;
#[macro_use]
extern crate log as log_crate;

#[macro_use]
mod macros;

mod analysis;
pub mod context;
pub mod graph;
pub mod ir;
pub mod iter;
pub mod log;
pub mod paths;
pub mod prelude;
pub mod report;
pub mod utils;
pub mod visitor;

use rustc_middle::ty::TyCtxt;

use crate::analysis::{SendSyncVarianceChecker, UnsafeDataflowChecker, UnsafeDestructorChecker};
use crate::context::RudraCtxtOwner;
use crate::log::Verbosity;
use crate::report::ReportLevel;

// Insert rustc arguments at the beginning of the argument list that Rudra wants to be
// set per default, for maximal validation power.
pub static RUDRA_DEFAULT_ARGS: &[&str] =
    &["-Zalways-encode-mir", "-Zmir-opt-level=0", "--cfg=rudra"];

#[derive(Debug, Clone, Copy)]
pub struct RudraConfig {
    pub verbosity: Verbosity,
    pub report_level: ReportLevel,
    pub unsafe_destructor_enabled: bool,
    pub send_sync_variance_enabled: bool,
    pub unsafe_dataflow_enabled: bool,
}

impl Default for RudraConfig {
    fn default() -> Self {
        RudraConfig {
            verbosity: Verbosity::Normal,
            report_level: ReportLevel::Info,
            unsafe_destructor_enabled: false,
            send_sync_variance_enabled: true,
            unsafe_dataflow_enabled: true,
        }
    }
}

/// Returns the "default sysroot" that Rudra will use if no `--sysroot` flag is set.
/// Should be a compile-time constant.
pub fn compile_time_sysroot() -> Option<String> {
    // option_env! is replaced to a constant at compile time
    if option_env!("RUSTC_STAGE").is_some() {
        // This is being built as part of rustc, and gets shipped with rustup.
        // We can rely on the sysroot computation in librustc.
        return None;
    }

    // For builds outside rustc, we need to ensure that we got a sysroot
    // that gets used as a default. The sysroot computation in librustc would
    // end up somewhere in the build dir.
    // Taken from PR <https://github.com/Manishearth/rust-clippy/pull/911>.
    let home = option_env!("RUSTUP_HOME").or(option_env!("MULTIRUST_HOME"));
    let toolchain = option_env!("RUSTUP_TOOLCHAIN").or(option_env!("MULTIRUST_TOOLCHAIN"));
    Some(match (home, toolchain) {
        (Some(home), Some(toolchain)) => format!("{}/toolchains/{}", home, toolchain),
        _ => option_env!("RUST_SYSROOT")
            .expect("To build Rudra without rustup, set the `RUST_SYSROOT` env var at build time")
            .to_owned(),
    })
}

fn run_analysis<F, R>(name: &str, f: F) -> R
where
    F: FnOnce() -> R,
{
    progress_info!("{} analysis started", name);
    let result = f();
    progress_info!("{} analysis finished", name);
    result
}

pub fn analyze<'tcx>(tcx: TyCtxt<'tcx>, config: RudraConfig) {
    // workaround to mimic arena lifetime
    let rcx_owner = RudraCtxtOwner::new(tcx, config.report_level);
    let rcx = &*Box::leak(Box::new(rcx_owner));

    // shadow the variable tcx
    #[allow(unused_variables)]
    let tcx = ();

    // Unsafe destructor analysis
    if config.unsafe_destructor_enabled {
        run_analysis("UnsafeDestructor", || {
            let mut checker = UnsafeDestructorChecker::new(rcx);
            checker.analyze();
        })
    }

    // Send/Sync variance analysis
    if config.send_sync_variance_enabled {
        run_analysis("SendSyncVariance", || {
            let checker = SendSyncVarianceChecker::new(rcx);
            checker.analyze();
        })
    }

    // Unsafe dataflow analysis
    if config.unsafe_dataflow_enabled {
        run_analysis("UnsafeDataflow", || {
            let checker = UnsafeDataflowChecker::new(rcx);
            checker.analyze();
        })
    }
}
