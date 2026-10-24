//! `sign <file>...`: writes `<file>.sig` beside each, the updater signature `tauri signer sign`
//! wrote (base64 of a prehashed minisign signature, same comments), which installed copies
//! check against PUBLIC_KEY. The key comes as tauri-cli took it: TAURI_SIGNING_PRIVATE_KEY,
//! the key itself (base64) or a path to it, and TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
//! release.yml builds it before any secret is in reach and runs it in the signing step.

use std::path::Path;
use std::process::ExitCode;

use base64::Engine as _;

fn main() -> ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: TAURI_SIGNING_PRIVATE_KEY=… sign <file>...");
        return ExitCode::from(2);
    }
    let key = match secret_key() {
        Ok(key) => key,
        Err(err) => {
            eprintln!("sign: {err}");
            return ExitCode::FAILURE;
        }
    };
    for file in files {
        if let Err(err) = sign(&key, Path::new(&file)) {
            eprintln!("sign: {file}: {err}");
            return ExitCode::FAILURE;
        }
        println!("signed {file}");
    }
    ExitCode::SUCCESS
}

fn secret_key() -> Result<minisign::SecretKey, String> {
    let value = std::env::var("TAURI_SIGNING_PRIVATE_KEY")
        .map_err(|_| "TAURI_SIGNING_PRIVATE_KEY isn't set")?;
    let encoded = if Path::new(&value).is_file() {
        std::fs::read_to_string(&value).map_err(|e| e.to_string())?
    } else {
        value
    };
    let text = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or("TAURI_SIGNING_PRIVATE_KEY isn't a base64 minisign secret key")?;
    let sk_box = minisign::SecretKeyBox::from_string(&text).map_err(|e| e.to_string())?;
    // Some(..) always: None would ask for the password on the terminal.
    let password = std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
    sk_box
        .clone()
        .into_secret_key(Some(password))
        .or_else(|err| sk_box.into_unencrypted_secret_key().map_err(|_| err))
        .map_err(|e| format!("the updater key: {e}"))
}

fn sign(key: &minisign::SecretKey, file: &Path) -> Result<(), String> {
    let name = file.file_name().ok_or("not a file")?.to_string_lossy();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let data = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let signature = minisign::sign(
        None,
        key,
        std::io::BufReader::new(data),
        Some(&format!("timestamp:{timestamp}\tfile:{name}")),
        Some("signature from tauri secret key"),
    )
    .map_err(|e| e.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(signature.to_string());
    std::fs::write(format!("{}.sig", file.display()), encoded).map_err(|e| e.to_string())
}
