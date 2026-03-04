use std::{io, path::Path, process::Command};

use serde::Deserialize;

// TODO: make it configurable via env variable
const COMMAND: &str = "verus";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Output {
    pub verification_results: VerificationResults,
}

impl Output {
    pub fn success(&self) -> bool {
        !self.verification_results.encountered_error
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct VerificationResults {
    pub encountered_error: bool,
    pub encountered_vir_error: bool,
    pub success: Option<bool>,
    pub verified: Option<u64>,
    pub errors: Option<u64>,
    pub is_verifying_entire_crate: Option<bool>,
}

pub fn verus_found() -> bool {
    Command::new(COMMAND).arg("--version").output().is_ok()
}

pub fn verify_file(path: impl AsRef<Path>) -> io::Result<Output> {
    let out = Command::new(COMMAND)
        .arg(path.as_ref())
        .arg("--output-json")
        .output()?;

    Ok(serde_json::from_slice(&out.stdout).expect("Failed to parse output"))
}
