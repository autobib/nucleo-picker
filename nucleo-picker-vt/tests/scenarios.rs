use std::time::Duration;

use nucleo_picker::{
    PickerOptions,
    event::{Event, PickerStatus},
};
use nucleo_picker_vt::{Driver, Error, PaneSnapshot};

#[derive(serde::Serialize)]
struct SnapshotOrigin<'a> {
    resolved_name: &'a str,
    name_expression: &'static str,
    sequence: usize,
}

macro_rules! checkpoint {
    ($runner:expr, $name:expr) => {{
        let name = $name;
        let resolved_name: &str = name.as_ref();
        let origin = crate::SnapshotOrigin {
            resolved_name,
            name_expression: stringify!($name),
            sequence: $runner.checkpoint_sequence(),
        };
        insta::with_settings!({ info => &origin, snapshot_suffix => resolved_name }, {
            insta::assert_yaml_snapshot!(
                $runner.scenario_name(),
                $runner.checkpoint(resolved_name.to_owned())?
            );
            Ok::<(), nucleo_picker_vt::Error>(())
        })?;
    }};
}

#[path = "scenarios/cancellation.rs"]
mod cancellation;
#[path = "scenarios/default.rs"]
mod default;
#[path = "scenarios/exceptional_events.rs"]
mod exceptional_events;
#[path = "scenarios/multi_select.rs"]
mod multi_select;
#[path = "scenarios/multiline.rs"]
mod multiline;
#[path = "scenarios/picker_options.rs"]
mod picker_options;
#[path = "scenarios/unicode.rs"]
mod unicode;

const WAIT: Duration = Duration::from_secs(5);
struct ScenarioRunner {
    scenario_name: &'static str,
    driver: Driver,
    timeout: Duration,
    checkpoint_sequence: usize,
}

impl ScenarioRunner {
    fn start_with_options<T: Into<String>>(
        scenario_name: &'static str,
        items: Vec<T>,
        options: PickerOptions,
    ) -> Self {
        crossterm::style::force_color_output(true);
        Self {
            scenario_name,
            driver: Driver::start_with_options(items, options),
            timeout: WAIT,
            checkpoint_sequence: 0,
        }
    }

    fn start_multi_with_options<T: Into<String>>(
        scenario_name: &'static str,
        items: Vec<T>,
        options: PickerOptions,
    ) -> Self {
        crossterm::style::force_color_output(true);
        Self {
            scenario_name,
            driver: Driver::start_multi_with_options(items, options),
            timeout: WAIT,
            checkpoint_sequence: 0,
        }
    }

    fn send(&self, event: Event) -> Result<(), Error> {
        self.driver.send(event)
    }

    fn type_text(&self, text: &str) -> Result<(), Error> {
        self.driver.type_text(text)
    }

    fn wait_for(
        &mut self,
        predicate: impl FnMut(&PickerStatus) -> bool,
    ) -> Result<PickerStatus, Error> {
        self.driver.wait_for(self.timeout, predicate)
    }

    fn wait_for_match_complete(&mut self, matched: u32, total: u32) -> Result<PickerStatus, Error> {
        self.driver
            .wait_for_match_complete(self.timeout, matched, total)
    }

    fn set_dimensions(&mut self, width: u16, height: u16) -> Result<(), Error> {
        self.driver.resize(width, height)
    }

    fn checkpoint(&mut self, name: impl Into<String>) -> Result<PaneSnapshot, Error> {
        self.checkpoint_sequence += 1;
        let name = name.into();
        self.driver.checkpoint(self.timeout, name)
    }

    fn checkpoint_sequence(&self) -> usize {
        self.checkpoint_sequence
    }

    fn scenario_name(&self) -> &'static str {
        self.scenario_name
    }

    fn finish(self) -> Result<Vec<String>, Error> {
        self.driver.finish(self.timeout)
    }
}

fn lines() -> Vec<&'static str> {
    vec![
        "item-00 alpha",
        "item-01 bravo",
        "item-02 charlie",
        "item-03 delta",
        "item-04 echo",
        "item-05 foxtrot",
        "item-06 golf",
        "item-07 hotel",
        "item-08 india",
        "item-09 juliet",
        "item-10 kilo",
        "item-11 lima",
        "item-12 mike",
        "item-13 november",
        "item-14 oscar",
        "item-15 papa",
        "item-16 quebec",
        "item-17 romeo",
        "item-18 sierra",
        "item-19 tango",
        "item-20 uniform",
        "item-21 victor",
        "item-22 whiskey",
        "item-23 xray",
    ]
}

fn unicode_lines() -> Vec<&'static str> {
    vec![
        "latin NFC café déjà-vu Ångström — long prefix before final",
        "latin NFD café déjà-vu Ångström — long prefix before final",
        "한국어 서울 한글 검색 — 아주 긴 접두사 뒤의 종착점 final",
        "조선말 평양 문화 언어 — 폭이 넓은 글자 뒤의 종착점 final",
        "日本語 東京 ひらがな カタカナ — 長い接頭辞の後の終点 final",
        "日本語 京都 漢字かな交じり文 — 長い接頭辞の後の終点 final",
        "emoji family 👨‍👩‍👧‍👦 — joined family before final",
        "emoji profession 👩🏽‍💻 — skin-tone technologist before final",
        "emoji flag 🏳️‍🌈 — variation-selector flag before final",
        "mixed café 서울 東京 — every retained family before final",
    ]
}
