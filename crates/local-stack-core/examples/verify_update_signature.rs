use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};

fn decode_tauri_value(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let encoded = fs::read_to_string(path)?;
    Ok(String::from_utf8(STANDARD.decode(encoded.trim())?)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let artifact = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: verify_update_signature <artifact> <signature> [public-key]")?;
    let signature_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: verify_update_signature <artifact> <signature> [public-key]")?;
    let public_key_path = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("updater/public.key"));
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }

    let public_key = PublicKey::decode(&decode_tauri_value(&public_key_path)?)?;
    let signature = Signature::decode(&decode_tauri_value(&signature_path)?)?;
    let mut verifier = public_key.verify_stream(&signature)?;
    let mut input = fs::File::open(&artifact)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        verifier.update(&buffer[..count]);
    }
    verifier.finalize()?;

    let size = fs::metadata(&artifact)?.len();
    println!(
        "Verified {} bytes from {} with the updater public key",
        size,
        artifact.display()
    );
    Ok(())
}
