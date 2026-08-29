use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

use crate::PaneSnapshot;

#[derive(Debug)]
pub struct Snap {
    pub header: Header,
    pub pane: PaneSnapshot,
}

#[derive(Debug, Deserialize)]
pub struct Header {
    pub source: String,
    pub expression: String,
    pub info: Info,
}

#[derive(Debug, Deserialize)]
pub struct Info {
    pub resolved_name: String,
    pub name_expression: String,
    pub sequence: usize,
}

#[derive(Debug)]
pub enum Error {
    MissingHeader,
    MissingPane,
    TrailingDocument,
    Yaml(serde_yaml_ng::Error),
}

impl FromStr for Snap {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.trim().is_empty() {
            return Err(Error::MissingHeader);
        }
        let mut documents = serde_yaml_ng::Deserializer::from_str(input);
        let header = Header::deserialize(documents.next().ok_or(Error::MissingHeader)?)?;
        let pane = PaneSnapshot::deserialize(documents.next().ok_or(Error::MissingPane)?)?;
        if documents.next().is_some() {
            return Err(Error::TrailingDocument);
        }
        Ok(Self { header, pane })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHeader => formatter.write_str("snapshot is missing its header"),
            Self::MissingPane => formatter.write_str("snapshot is missing its pane"),
            Self::TrailingDocument => {
                formatter.write_str("snapshot contains more than two YAML documents")
            }
            Self::Yaml(error) => error.fmt(formatter),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Yaml(error) => Some(error),
            Self::MissingHeader | Self::MissingPane | Self::TrailingDocument => None,
        }
    }
}

impl From<serde_yaml_ng::Error> for Error {
    fn from(error: serde_yaml_ng::Error) -> Self {
        Self::Yaml(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = include_str!(
        "../tests/scenarios/snapshots/scenarios__multiline__multiline_default@default-height-60x1.snap"
    );

    #[test]
    fn parses_header_and_pane_as_separate_yaml_documents() {
        let snap: Snap = SNAPSHOT.parse().unwrap();

        assert_eq!(
            snap.header.source,
            "nucleo-picker-vt/tests/scenarios/multiline.rs"
        );
        assert_eq!(
            snap.header.expression,
            "sr.checkpoint(resolved_name.to_owned())?"
        );
        assert_eq!(snap.header.info.resolved_name, "default-height-60x1");
        assert_eq!(
            snap.header.info.name_expression,
            "format!(\"default-height-60x{rows}\")"
        );
        assert_eq!(snap.header.info.sequence, 5);
        assert_eq!(snap.pane.size.cols, 60);
        assert_eq!(snap.pane.size.rows, 1);
    }

    #[test]
    fn allows_document_marker_text_inside_header_scalars() {
        let input = SNAPSHOT.replacen(
            "expression: sr.checkpoint(resolved_name.to_owned())?",
            "expression: |-\n  sr.checkpoint(resolved_name.to_owned())?\n  ---",
            1,
        );

        let snap: Snap = input.parse().unwrap();

        assert_eq!(
            snap.header.expression,
            "sr.checkpoint(resolved_name.to_owned())?\n---"
        );
    }

    #[test]
    fn rejects_missing_yaml_documents() {
        assert!(matches!("".parse::<Snap>(), Err(Error::MissingHeader)));
        assert!(matches!(
            "---\nsource: test\nexpression: test\ninfo:\n  resolved_name: test\n  name_expression: test\n  sequence: 0\n"
                .parse::<Snap>(),
            Err(Error::MissingPane)
        ));
    }

    #[test]
    fn rejects_trailing_yaml_documents() {
        let input = format!("{SNAPSHOT}\n---\nunexpected: document\n");

        assert!(matches!(
            input.parse::<Snap>(),
            Err(Error::TrailingDocument)
        ));
    }
}
