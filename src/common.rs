#![allow(dead_code)]

pub use clap::Parser;
pub use log::{info, warn};

pub use data_beans::sparse_data_visitors::*;
pub use data_beans::sparse_io::*;
pub use data_beans::sparse_io_vector::*;

pub type Mat = nalgebra::DMatrix<f32>;
pub type DVec = nalgebra::DVector<f32>;
pub type CscMat = nalgebra_sparse::CscMatrix<f32>;
pub type SparseData = dyn SparseIo<IndexIter = Vec<usize>>;

pub struct EmptyArg {}

/// The files a simulator always writes for prefix `output`: the counts, and
/// the files named `beside` it.
pub fn sim_output_files(
    output: &str,
    zip: bool,
    backend: &SparseIoBackend,
    beside: &[&str],
) -> anyhow::Result<Vec<String>> {
    let counts = data_beans::zarr_io::output_target(output, backend.clone(), zip)?.found;
    let beside = beside.iter().map(|s| format!("{output}.{s}"));
    Ok(std::iter::once(counts.into()).chain(beside).collect())
}
