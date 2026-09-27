#![forbid(unsafe_code)]

mod feedback;
mod ingest;
mod interface;
mod profile;
pub mod protocol;
mod tui;
mod worker;

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
    match args.next().as_deref() {
        Some("ingest") => run_ingest(args),
        Some("tui") => run_tui(args),
        Some("feedback") => run_feedback(args),
        Some("profile") => run_profile(args),
        _ => Err(invalid_input(usage())),
    }
}

fn run_ingest(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut manifest = None;
    let mut data_dir = None;
    while let Some(flag) = args.next() {
        let value = option_value(&mut args, &flag)?;
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

fn run_tui(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut data_dir = None;
    let mut python = None;
    let mut feedback_enabled = false;
    while let Some(flag) = args.next() {
        if flag == "--feedback" {
            if feedback_enabled {
                return Err(invalid_input("duplicate option: --feedback"));
            }
            feedback_enabled = true;
            continue;
        }
        let value = option_value(&mut args, &flag)?;
        match flag.as_str() {
            "--data-dir" if data_dir.is_none() => data_dir = Some(PathBuf::from(value)),
            "--python" if python.is_none() => python = Some(PathBuf::from(value)),
            "--data-dir" | "--python" => {
                return Err(invalid_input(format!("duplicate option: {flag}")));
            }
            _ => return Err(invalid_input(format!("unknown option: {flag}"))),
        }
    }
    let data_dir = data_dir.ok_or_else(|| invalid_input("missing --data-dir"))?;
    if let Some(selected_id) = interface::run(interface::Options {
        data_dir,
        python,
        feedback_enabled,
    })? {
        println!("{}", serde_json::json!({ "selected_id": selected_id }));
    }
    Ok(())
}

fn run_feedback(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let action = args
        .next()
        .ok_or_else(|| invalid_input("feedback requires inspect or delete"))?;
    if action != "inspect" && action != "delete" {
        return Err(invalid_input("feedback action must be inspect or delete"));
    }
    let mut data_dir = None;
    while let Some(flag) = args.next() {
        let value = option_value(&mut args, &flag)?;
        match flag.as_str() {
            "--data-dir" if data_dir.is_none() => data_dir = Some(PathBuf::from(value)),
            "--data-dir" => return Err(invalid_input("duplicate option: --data-dir")),
            _ => return Err(invalid_input(format!("unknown option: {flag}"))),
        }
    }
    let data_dir = data_dir.ok_or_else(|| invalid_input("missing --data-dir"))?;
    let log = feedback::FeedbackLog::new(interface::feedback_path(&data_dir), false)?;
    if action == "inspect" {
        log.cleanup_expired()?;
        println!("{}", serde_json::to_string_pretty(&log.read()?)?);
    } else if log.delete()? {
        println!("deleted {}", log.path().display());
    } else {
        println!("no feedback file at {}", log.path().display());
    }
    Ok(())
}

fn run_profile(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut data_dir = None;
    let mut python = None;
    let mut benchmark = None;
    let mut output = None;
    let mut warmups = 128;
    let mut measurements = 1_024;
    while let Some(flag) = args.next() {
        let value = option_value(&mut args, &flag)?;
        match flag.as_str() {
            "--data-dir" if data_dir.is_none() => data_dir = Some(PathBuf::from(value)),
            "--python" if python.is_none() => python = Some(PathBuf::from(value)),
            "--benchmark" if benchmark.is_none() => benchmark = Some(PathBuf::from(value)),
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            "--warmups" => warmups = value.parse()?,
            "--measurements" => measurements = value.parse()?,
            "--data-dir" | "--python" | "--benchmark" | "--output" => {
                return Err(invalid_input(format!("duplicate option: {flag}")));
            }
            _ => return Err(invalid_input(format!("unknown option: {flag}"))),
        }
    }
    profile::run(profile::Options {
        data_dir: data_dir.ok_or_else(|| invalid_input("missing --data-dir"))?,
        python,
        benchmark: benchmark.ok_or_else(|| invalid_input("missing --benchmark"))?,
        output: output.ok_or_else(|| invalid_input("missing --output"))?,
        warmups,
        measurements,
    })
}

fn option_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    args.next()
        .ok_or_else(|| invalid_input(format!("missing value for {flag}")))
}

fn usage() -> &'static str {
    "usage:\n  mambomeme ingest --manifest PATH --data-dir DIR\n  mambomeme tui --data-dir DIR [--python PATH] [--feedback]\n  mambomeme feedback inspect|delete --data-dir DIR\n  mambomeme profile --data-dir DIR --benchmark PATH --output PATH [--python PATH] [--warmups N] [--measurements N]"
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
