use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use clap::Parser;
use perf_event as prf;
use prf::{events::Hardware, CountAndTime};

#[derive(clap::Subcommand, Debug)]
enum Command {
    Record(RecordCliOptions),
    Compare(CompareCliOptions),
}

#[derive(Debug, clap::Args)]
struct RecordCliOptions {
    command: String,
    output_file: PathBuf,
    wat_files: Vec<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct CompareCliOptions {
    base_file: PathBuf,
    compared_file: PathBuf,
}

#[derive(clap::Parser)]
struct CliOptions {
    #[command(subcommand)]
    command: Command,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Measure {
    ref_cycles: u64,
    instructions: u64,
    cpu_time: u64,
}

fn scale(
    CountAndTime {
        count,
        time_enabled,
        time_running,
    }: CountAndTime,
) -> u64 {
    if time_running < time_enabled {
        ((count as u128) * (time_enabled as u128) / (time_running as u128)) as u64
    } else {
        count
    }
}

fn record(cli_options: RecordCliOptions) -> anyhow::Result<()> {
    let mut ref_cycles = prf::Builder::new(Hardware::REF_CPU_CYCLES)
        .inherit(true)
        .enable_on_exec(true)
        .build()?;
    let mut instructions = prf::Builder::new(Hardware::INSTRUCTIONS)
        .inherit(true)
        .enable_on_exec(true)
        .build()?;
    let mut res = HashMap::new();
    for wat_file in &cli_options.wat_files {
        let mut command_words = cli_options.command.split_whitespace();
        let command = command_words.next().expect("Non-empty command");
        let mut command = std::process::Command::new(command);
        command.args(command_words);
        command.arg(wat_file);
        for c in [&mut ref_cycles, &mut instructions] {
            c.reset()?;
        }
        command.status()?;
        for c in [&mut ref_cycles, &mut instructions] {
            c.disable()?
        }
        let meas = Measure {
            ref_cycles: scale(ref_cycles.read_count_and_time()?),
            instructions: scale(instructions.read_count_and_time()?),
            cpu_time: ref_cycles.read_count_and_time()?.time_enabled,
        };
        res.insert(wat_file, meas);
    }
    let output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(cli_options.output_file)?;
    serde_json::to_writer_pretty(output, &res)?;
    let mut table = prettytable::Table::new();
    table.add_row(prettytable::row![
        "File",
        "Ref-cycles",
        "Instructions",
        "CPU Time (ms)"
    ]);
    for (input_file, meas) in res {
        table.add_row(prettytable::row![
            input_file.display(),
            meas.ref_cycles,
            meas.instructions,
            meas.cpu_time
        ]);
    }
    table.printstd();
    Ok(())
}

fn compare(cli_options: CompareCliOptions) -> anyhow::Result<()> {
    let base_file = std::fs::read_to_string(cli_options.base_file)?;
    let compared_file = std::fs::read_to_string(cli_options.compared_file)?;
    let base: HashMap<&Path, Measure> = serde_json::from_str(&base_file)?;
    let compared: HashMap<&Path, Measure> = serde_json::from_str(&compared_file)?;
    let mut table = prettytable::Table::new();
    table.add_row(prettytable::row![
        "File",
        "Ref-cycles",
        "Instructions",
        "CPU Time (ms)"
    ]);
    fn rel_diff(base: u64, compared: u64) -> prettytable::Cell {
        let diff = (((compared as f64) - (base as f64)) * 100.) / (base as f64);
        let mut cell = prettytable::Cell::new(&format!("{diff:+.1}%",));
        if diff > 0.1 {
            cell.style(prettytable::Attr::ForegroundColor(prettytable::color::RED));
        } else if diff < -0.1 {
            cell.style(prettytable::Attr::ForegroundColor(
                prettytable::color::GREEN,
            ));
        }
        cell
    }
    for (&key, base_measure) in &base {
        let Some(compared_measure) = compared.get(key) else { continue };
        table.add_row(prettytable::Row::new(vec![
            prettytable::Cell::new(&key.display().to_string()),
            rel_diff(base_measure.ref_cycles, compared_measure.ref_cycles),
            rel_diff(base_measure.instructions, compared_measure.instructions),
            rel_diff(base_measure.cpu_time, compared_measure.cpu_time),
        ]));
    }
    table.printstd();
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    // let mut counter_group = prf::Group::new()?;
    let cli_options = CliOptions::parse();
    match cli_options.command {
        Command::Record(cli_options) => record(cli_options),
        Command::Compare(cli_options) => compare(cli_options),
    }
}
