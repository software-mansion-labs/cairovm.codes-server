use anyhow::{Context, Result};
use std::{path::Path, process::Command};

pub fn prove_and_verify(project_path: &Path) -> Result<(String, u64, u64)> {
    let prover_path_str =
        std::env::var("PROVER_PATH").with_context(|| "PROVER_PATH environment variable not set")?;
    let prover_path = Path::new(&prover_path_str);
    let output = Command::new(prover_path)
        .arg(project_path)
        .output()
        .with_context(|| "Failed to run prover")?;

    if !output.status.success() {
        anyhow::bail!("Prover exited with status: {}", output.status);
    }

    // Read the proof.json file
    let proof_path = project_path.join("proof.json");
    let proof_content = std::fs::read_to_string(&proof_path)
        .with_context(|| format!("Failed to read proof.json from {:?}", proof_path))?;

    let stdout =
        String::from_utf8(output.stdout).with_context(|| "Prover output was not valid UTF-8")?;

    // Expect two comma separated integers: proving_time,verification_time.
    let parts: Vec<&str> = stdout.trim().split(',').collect();
    if parts.len() != 2 {
        anyhow::bail!("Unexpected prover output: {}", stdout);
    }

    let proving_time: u64 = parts[0]
        .parse()
        .with_context(|| format!("Failed to parse proving time from '{}'", parts[0]))?;
    let verification_time: u64 = parts[1]
        .parse()
        .with_context(|| format!("Failed to parse verification time from '{}'", parts[1]))?;

    Ok((proof_content, proving_time, verification_time))
}
