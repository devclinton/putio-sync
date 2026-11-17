//! `make-patch <old> <new> <out>`: writes the bsdiff patch from `old` to `new`, uncompressed.
//! The release workflow and `make release-patches` compress it with `xz -9e`
//! (contracts/release-assets.md).

use std::{env, fs, io::BufWriter, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [old, new, out] = args.as_slice() else {
        eprintln!("usage: make-patch <old> <new> <out>");
        return ExitCode::from(2);
    };
    match make_patch(old, new, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("make-patch: {e}");
            ExitCode::FAILURE
        }
    }
}

fn make_patch(old: &str, new: &str, out: &str) -> std::io::Result<()> {
    let old = fs::read(old)?;
    let new = fs::read(new)?;
    let mut writer = BufWriter::new(fs::File::create(out)?);
    bsdiff::diff(&old, &new, &mut writer)?;
    writer.into_inner()?.sync_all()
}
