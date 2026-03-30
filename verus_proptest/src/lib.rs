#![allow(dead_code)]

use std::{
    io::{self},
    path::Path,
};

use databake::Bake;
use proptest::{
    prelude::*,
    test_runner::{Config, TestCaseResult, TestError, TestRunner},
};
use rand::{RngExt, distr::Alphanumeric, rngs::ThreadRng};
use tempfile::{TempDir, TempPath};
pub use verus_proptest_macro::*;

pub use crate::codegen::{EnsuresCodegen, RequiresCodegen};

#[doc(hidden)]
pub mod hidden {
    pub use proptest;
}

mod codegen;
mod verus;

#[derive(Debug, Clone, Copy)]
pub enum Ref {
    Ref,
    Mut,
}

#[derive(Debug, Clone)]
pub struct RefStack(pub &'static [Ref]);

#[derive(Debug, Clone)]
pub struct Arg {
    pub pattern: &'static str,
    pub ref_stack: RefStack,
}

pub struct Args(pub &'static [Arg]);

/// - Macro time
///     - parse fn
///     - get args type
///     - generate `Testable` shim
/// - Runtime
///     - generate args with arbitrary
///     - bake into test file
///     - verify `requires`,
///         - if verified, run proptest, get return value
///         - otherwise reject
///     - verify `ensures`
///     - shrink (by proptest)
pub trait Testable {
    type Args: Bake + Arbitrary + Clone;
    type Ret: Bake;

    const ARGS: Args;
    const RET: Option<&str> = None;
    const RET_TYPE: Option<&str> = None;
    const REQUIRES: Option<&str> = None;
    const ENSURES: Option<&str> = None;

    fn run(args: Self::Args) -> Self::Ret;

    fn strategy() -> BoxedStrategy<Self::Args>
    where
        <Self::Args as Arbitrary>::Strategy: 'static,
    {
        any::<Self::Args>().boxed()
    }
}

pub fn test<T: Testable>() -> Result<(), TestError<T::Args>>
where
    <T::Args as Arbitrary>::Strategy: 'static,
{
    let mut runner = TestRunner::new(Config {
        verbose: 2,
        ..Default::default()
    });
    let s = T::strategy();
    let dir = TempDir::with_prefix("verus_proptest").map_err(io_err_to_abort)?;
    println!("Temp dir: {}", dir.path().display());
    runner.run(&s, move |val| run::<T>(dir.path(), val))
}

fn run<T: Testable>(dir: &Path, args: T::Args) -> TestCaseResult {
    let req_codegen = RequiresCodegen::<T>::new(&args);

    if let Some(reqs) = req_codegen.codegen() {
        let path = TempPath::from_path(dir.join(random_filename("requires_")));

        std::fs::write(&path, reqs.to_string().as_bytes()).map_err(io_err_to_fail)?;
        let output = verus::verify_file(path).map_err(io_err_to_fail)?;

        if !output.success() {
            return Err(TestCaseError::Reject("verus verification failed".into()));
        }
    }

    let res = T::run(args.clone());

    if let Some(ensures) = EnsuresCodegen::new(req_codegen, &res).codegen() {
        let path = TempPath::from_path(dir.join(random_filename("ensures_")));

        std::fs::write(&path, ensures.to_string().as_bytes()).map_err(io_err_to_fail)?;
        let output = verus::verify_file(path).map_err(io_err_to_fail)?;

        if !output.success() {
            return Err(TestCaseError::Fail("verus verification failed".into()));
        }
    }

    Ok(())
}

fn random_filename(prefix: &str) -> String {
    let rng = ThreadRng::default();
    prefix
        .chars()
        .chain(rng.sample_iter(Alphanumeric).map(char::from).take(8))
        .chain(".rs".chars())
        .collect()
}

fn io_err_to_abort<T>(err: io::Error) -> TestError<T> {
    TestError::Abort(format!("Cannot open temp dir: {err}").into())
}

fn io_err_to_fail(err: io::Error) -> TestCaseError {
    TestCaseError::Fail(format!("Cannot open temp file: {err}").into())
}
