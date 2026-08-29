use std::{
    collections::BTreeMap,
    error::Error as StdError,
    ffi::OsStr,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use nucleo_picker_vt::snap::{self, Snap};
use xml::escape::escape_str_pcdata;

type Summary = BTreeMap<String, Vec<Snap>>;
type Diffs = BTreeMap<String, Vec<Diff>>;

#[derive(Debug)]
struct Diff {
    baseline: Option<Snap>,
    pending: Snap,
}

#[derive(Debug)]
pub(crate) enum Error {
    Directory { path: PathBuf, source: io::Error },
    File { path: PathBuf, source: io::Error },
    Snapshot { path: PathBuf, source: snap::Error },
    Scenario { path: PathBuf },
    PairMismatch { baseline: PathBuf, pending: PathBuf },
    Write(io::Error),
    Svg(xml::writer::Error),
}

pub(crate) fn write_diff<W: Write>(directory: &Path, output: &mut W) -> Result<(), Error> {
    let diffs = load_diffs(directory)?;
    write_document_start(output, "Snapshot diff")?;
    for (scenario, entries) in diffs {
        write_group_start(output, &scenario, entries.len())?;
        for entry in entries {
            write_entry_start(
                output,
                &entry.pending.header.info.resolved_name,
                entry.baseline.is_some(),
            )?;
            if let Some(baseline) = entry.baseline {
                write_pane(output, "Baseline", "snap", &baseline)?;
            }
            write_pane(output, "Pending", "snap-new", &entry.pending)?;
            output.write_all(b"</div></article>")?;
        }
        output.write_all(b"</div></details>")?;
    }
    write_document_end(output)
}

pub(crate) fn write_summary<W: Write>(directory: &Path, output: &mut W) -> Result<(), Error> {
    let summary = load_summary(directory)?;
    write_document_start(output, "Snapshot summary")?;
    for (scenario, entries) in summary {
        write_group_start(output, &scenario, entries.len())?;
        for entry in entries {
            write_entry_start(output, &entry.header.info.resolved_name, false)?;
            write_pane(output, "Snapshot", "snap", &entry)?;
            output.write_all(b"</div></article>")?;
        }
        output.write_all(b"</div></details>")?;
    }
    write_document_end(output)
}

fn load_summary(directory: &Path) -> Result<Summary, Error> {
    let mut summary: Summary = BTreeMap::new();
    for path in snapshot_paths(directory)? {
        if path.extension() != Some(OsStr::new("snap")) {
            continue;
        }
        let snap = read_snap(&path)?;
        let scenario = scenario(&snap, &path)?;
        summary.entry(scenario).or_default().push(snap);
    }
    for entries in summary.values_mut() {
        entries.sort_by(|left, right| {
            left.header
                .info
                .sequence
                .cmp(&right.header.info.sequence)
                .then_with(|| {
                    left.header
                        .info
                        .resolved_name
                        .cmp(&right.header.info.resolved_name)
                })
        });
    }
    Ok(summary)
}

fn load_diffs(directory: &Path) -> Result<Diffs, Error> {
    let mut diffs: Diffs = BTreeMap::new();
    for pending_path in snapshot_paths(directory)? {
        let Some(file_name) = pending_path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(stem) = file_name.strip_suffix(".snap.new") else {
            continue;
        };
        let baseline_path = directory.join(format!("{stem}.snap"));
        let pending = read_snap(&pending_path)?;
        let pending_scenario = scenario(&pending, &pending_path)?;
        let baseline = if baseline_path.is_file() {
            let baseline = read_snap(&baseline_path)?;
            let baseline_scenario = scenario(&baseline, &baseline_path)?;
            if baseline_scenario != pending_scenario
                || baseline.header.info.resolved_name != pending.header.info.resolved_name
                || baseline.header.info.sequence != pending.header.info.sequence
            {
                return Err(Error::PairMismatch {
                    baseline: baseline_path,
                    pending: pending_path,
                });
            }
            Some(baseline)
        } else {
            None
        };
        diffs
            .entry(pending_scenario)
            .or_default()
            .push(Diff { baseline, pending });
    }
    for entries in diffs.values_mut() {
        entries.sort_by(|left, right| {
            left.pending
                .header
                .info
                .sequence
                .cmp(&right.pending.header.info.sequence)
                .then_with(|| {
                    left.pending
                        .header
                        .info
                        .resolved_name
                        .cmp(&right.pending.header.info.resolved_name)
                })
        });
    }
    Ok(diffs)
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries = fs::read_dir(directory).map_err(|source| Error::Directory {
        path: directory.to_owned(),
        source,
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Directory {
            path: directory.to_owned(),
            source,
        })?;
        if entry
            .file_type()
            .map_err(|source| Error::File {
                path: entry.path(),
                source,
            })?
            .is_file()
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_snap(path: &Path) -> Result<Snap, Error> {
    let input = fs::read_to_string(path).map_err(|source| Error::File {
        path: path.to_owned(),
        source,
    })?;
    input.parse().map_err(|source| Error::Snapshot {
        path: path.to_owned(),
        source,
    })
}

fn scenario(snap: &Snap, path: &Path) -> Result<String, Error> {
    Path::new(&snap.header.source)
        .file_stem()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Scenario {
            path: path.to_owned(),
        })
}

fn write_document_start<W: Write>(output: &mut W, title: &str) -> Result<(), Error> {
    write!(
        output,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{}</style></head><body><main><h1>{}</h1>",
        escape_str_pcdata(title),
        include_str!("report.css"),
        escape_str_pcdata(title),
    )?;
    Ok(())
}

fn write_document_end<W: Write>(output: &mut W) -> Result<(), Error> {
    output.write_all(b"</main></body></html>")?;
    Ok(())
}

fn write_group_start<W: Write>(output: &mut W, scenario: &str, count: usize) -> Result<(), Error> {
    write!(
        output,
        "<details><summary>{} <span>{count}</span></summary><div class=\"scenario\">",
        escape_str_pcdata(scenario),
    )?;
    Ok(())
}

fn write_entry_start<W: Write>(output: &mut W, name: &str, diff: bool) -> Result<(), Error> {
    let class = if diff { "panes diff" } else { "panes" };
    write!(
        output,
        "<article class=\"entry\"><h2>{}</h2><div class=\"{class}\">",
        escape_str_pcdata(name),
    )?;
    Ok(())
}

fn write_pane<W: Write>(
    output: &mut W,
    label: &str,
    class: &str,
    snap: &Snap,
) -> Result<(), Error> {
    write!(
        output,
        "<section class=\"pane {class}\"><h3>{}</h3><div class=\"viewport\">",
        escape_str_pcdata(label),
    )?;
    snap.pane.write_svg(&mut *output).map_err(Error::Svg)?;
    output.write_all(b"</div></section>")?;
    Ok(())
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory { path, .. } => {
                write!(formatter, "failed to read directory {}", path.display())
            }
            Self::File { path, .. } => write!(formatter, "failed to read {}", path.display()),
            Self::Snapshot { path, .. } => {
                write!(formatter, "failed to parse {}", path.display())
            }
            Self::Scenario { path } => {
                write!(
                    formatter,
                    "snapshot has no scenario name: {}",
                    path.display()
                )
            }
            Self::PairMismatch { baseline, pending } => write!(
                formatter,
                "snapshot pair metadata does not match: {} and {}",
                baseline.display(),
                pending.display()
            ),
            Self::Write(_) => formatter.write_str("failed to write report"),
            Self::Svg(_) => formatter.write_str("failed to write snapshot SVG"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Directory { source, .. } | Self::File { source, .. } | Self::Write(source) => {
                Some(source)
            }
            Self::Snapshot { source, .. } => Some(source),
            Self::Svg(source) => Some(source),
            Self::Scenario { .. } | Self::PairMismatch { .. } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Write(error)
    }
}
