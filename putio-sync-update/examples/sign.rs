//! `sign <secret-key> <file> <trusted-comment>`: writes `<file>.minisig`, reading the key's
//! password from `MINISIGN_PASSWORD`. `scripts/fake-release.sh` signs its `SHA256SUMS` with the
//! test key in `tests/data/` (password `putio-sync-test`); real releases are signed by the
//! `minisign` tool in `release-binaries.yml`.

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [key, file, comment] = args.as_slice() else {
        eprintln!("usage: sign <secret-key> <file> <trusted-comment>");
        return ExitCode::from(2);
    };
    match sign(key, file, comment) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sign: {e}");
            ExitCode::FAILURE
        }
    }
}

fn sign(key: &str, file: &str, comment: &str) -> Result<(), Box<dyn std::error::Error>> {
    let password = env::var("MINISIGN_PASSWORD").map_err(|_| "MINISIGN_PASSWORD is not set")?;
    let secret = minisign::SecretKey::from_file(key, Some(password))?;
    let data = fs::File::open(file)?;
    let signature = minisign::sign(None, &secret, data, Some(comment), None)?;
    fs::write(format!("{file}.minisig"), signature.into_string())?;
    Ok(())
}
