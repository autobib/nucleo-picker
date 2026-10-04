use super::{MatchListConfig, draw::RenderScratch, layout::ListLayout};

pub(crate) struct MatchListState {
    pub(crate) layout: ListLayout,
    pub(crate) config: MatchListConfig,
    pub(super) scratch: RenderScratch,
}

impl MatchListState {
    pub fn new(config: MatchListConfig, nucleo_config: nucleo::Config) -> Self {
        Self {
            layout: ListLayout::new(),
            config,
            scratch: RenderScratch::new(nucleo_config),
        }
    }
}
