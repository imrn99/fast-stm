//! STM benchmarks

#[cfg(all(feature = "fast-stm", feature = "sserp-stm"))]
compile_error!("features `fast-stm` and `sserp-stm` are mutually exclusive");

#[cfg(not(any(feature = "fast-stm", feature = "sserp-stm")))]
compile_error!("enable exactly one of `fast-stm` or `sserp-stm`");

#[cfg(feature = "fast-stm")]
pub use fast_stm as stm;

#[cfg(feature = "sserp-stm")]
pub use sserp_stm as stm;
