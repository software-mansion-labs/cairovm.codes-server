use anyhow::{Result, anyhow};
use prover::{
    cairo_air::{ProverConfig, prove_cairo, verify_cairo},
    stwo_prover::core::{pcs::PcsConfig, vcs::blake2_merkle::Blake2sMerkleChannel},
};
use std::{io::Write, path::Path, time::Instant};
use stwo_cairo_adapter::vm_import::adapt_vm_output;

pub fn main() -> Result<()> {
    // Retrieve command-line arguments
    let args: Vec<String> = std::env::args().collect();

    // Check if the folder path argument is provided
    if args.len() < 2 {
        return Err(anyhow!("Please provide the folder path as an argument."));
    }

    let folder_path = Path::new(&args[1]);

    let air_public_input_path = folder_path.join("air_public_input.txt");
    let air_private_input_path = folder_path.join("air_private_input.txt");

    let result = adapt_vm_output(&air_public_input_path, &air_private_input_path);

    // Track proving time
    let proving_start = Instant::now();
    let proof = prove_cairo::<Blake2sMerkleChannel>(
        result.unwrap(),
        ProverConfig::default(),
        PcsConfig::default(),
    )
    .unwrap();
    let proving_duration = proving_start.elapsed();

    // Serialize proof to JSON
    let proof_json = serde_json::to_string(&proof).unwrap();
    let proof_file_path = folder_path.join("proof.json");
    let mut file = std::fs::File::create(proof_file_path).unwrap();
    file.write_all(proof_json.as_bytes()).unwrap();

    let verifying_start = Instant::now();
    let verify_result = verify_cairo::<Blake2sMerkleChannel>(proof, PcsConfig::default());
    let verifying_duration = verifying_start.elapsed();

    println!(
        "{},{}",
        proving_duration.as_millis(),
        verifying_duration.as_millis()
    );

    Ok(())
}
