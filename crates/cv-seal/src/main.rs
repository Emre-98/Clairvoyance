//! `cv-seal keygen [file]`: makes a new key (writes the private key to `file`, prints the public key).
//! `cv-seal seal <exe>`: signs the executable in place with the private key in `CV_SEAL_KEY` (hex).
//!   Without `CV_SEAL_KEY` it does nothing (a dev build), unless `CV_SEAL_REQUIRED=1`. If
//!   `CV_SEAL_PUBKEY` is set, the result is verified against it (catches a mismatched secret).
//! `cv-seal verify <exe> <public key hex>`: checks a sealed executable.
//! Run by Tauri's `beforeBundleCommand` (app/tauri.conf.json) in the Release workflow; see RELEASING.md.

use std::process::ExitCode;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["keygen", rest @ ..] => {
            let seed = cv_seal::new_seed();
            let public = cv_seal::public_key(&seed).map_err(|e| e.to_string())?;
            match rest.first() {
                Some(path) => {
                    if std::path::Path::new(path).exists() {
                        return Err(format!("{path} already exists; not overwriting a key"));
                    }
                    std::fs::write(path, cv_seal::to_hex(&seed)).map_err(|e| format!("{path}: {e}"))?;
                    println!("Private key written to {path} (keep it secret, back it up).");
                }
                None => println!("Private key (CV_SEAL_KEY, keep it secret): {}", cv_seal::to_hex(&seed)),
            }
            println!("Public key (CV_SEAL_PUBKEY): {}", cv_seal::to_hex(&public));
            Ok(())
        }
        ["seal", exe] => {
            let required = env("CV_SEAL_REQUIRED").as_deref() == Some("1");
            let Some(key) = env("CV_SEAL_KEY") else {
                if required {
                    return Err("CV_SEAL_REQUIRED=1 but CV_SEAL_KEY isn't set: the build would refuse to start".into());
                }
                println!("cv-seal: no CV_SEAL_KEY, not sealing {exe} (dev build)");
                return Ok(());
            };
            let seed = cv_seal::from_hex::<32>(&key).ok_or("CV_SEAL_KEY must be 64 hex characters")?;
            let mut file = std::fs::read(exe).map_err(|e| format!("{exe}: {e}"))?;
            cv_seal::seal(&mut file, &seed).map_err(|e| format!("{exe}: {e}"))?;
            if let Some(pk) = env("CV_SEAL_PUBKEY") {
                let pk = cv_seal::from_hex::<32>(&pk).ok_or("CV_SEAL_PUBKEY must be 64 hex characters")?;
                cv_seal::verify(&file, &pk).map_err(|_| "CV_SEAL_KEY and CV_SEAL_PUBKEY aren't a pair")?;
            }
            std::fs::write(exe, &file).map_err(|e| format!("{exe}: {e}"))?;
            println!("cv-seal: sealed {exe}");
            Ok(())
        }
        ["verify", exe, pk] => {
            let pk = cv_seal::from_hex::<32>(pk).ok_or("public key must be 64 hex characters")?;
            let file = std::fs::read(exe).map_err(|e| format!("{exe}: {e}"))?;
            cv_seal::verify(&file, &pk).map_err(|e| format!("{exe}: {e}"))?;
            println!("cv-seal: {exe} is sealed and intact");
            Ok(())
        }
        _ => Err("usage: cv-seal keygen [file] | seal <exe> | verify <exe> <public key hex>".into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cv-seal: {e}");
            ExitCode::FAILURE
        }
    }
}
