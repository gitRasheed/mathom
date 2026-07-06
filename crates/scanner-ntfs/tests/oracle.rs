//! Parity tests against the `mft` crate oracle (stage 4). Only compiled with
//! `--features oracle-tests`; the oracle is never a runtime dependency.

// Force the oracle crate to be linked so feature wiring is verified even
// before the real parity suite lands.
use mft as _;
