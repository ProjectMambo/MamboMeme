#![forbid(unsafe_code)]

mod ingest;

use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("mambomeme: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("ingest") {
        return Err(invalid_input(
            "usage: mambomeme ingest --manifest PATH --data-dir DIR",
        ));
    }

    let mut manifest = None;
    let mut data_dir = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| invalid_input(format!("missing value for {flag}")))?;
        match flag.as_str() {
            "--manifest" if manifest.is_none() => manifest = Some(PathBuf::from(value)),
            "--data-dir" if data_dir.is_none() => data_dir = Some(PathBuf::from(value)),
            "--manifest" | "--data-dir" => {
                return Err(invalid_input(format!("duplicate option: {flag}")));
            }
            _ => return Err(invalid_input(format!("unknown option: {flag}"))),
        }
    }

    let manifest = manifest.ok_or_else(|| invalid_input("missing --manifest"))?;
    let data_dir = data_dir.ok_or_else(|| invalid_input("missing --data-dir"))?;
    let summary = ingest::ingest(&manifest, &data_dir)?;
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
