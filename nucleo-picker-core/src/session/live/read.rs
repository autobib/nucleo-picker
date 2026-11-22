use super::SessionState;
use crate::{
    Generation,
    interface::Interface,
    session::{Components, ItemRef, MatchStatus, Matches, QueueStatus},
};

pub(super) trait SessionRead {
    type Item: Send + Sync + 'static;
    type Renderer;
    type Interface: Interface<Self::Item>;

    fn session(&self) -> &SessionState<'_, Self::Item, Self::Renderer, Self::Interface>;

    fn generation(&self) -> Generation {
        self.session().engine.generation()
    }

    fn query(&self) -> &str {
        self.session().prompt.contents()
    }

    fn matches(&self) -> Matches<'_, Self::Item> {
        Matches::new(self.session().engine)
    }

    fn hovered_rank(&self) -> Option<u32> {
        let session = self.session();
        session.list.hovered_rank(session.engine)
    }

    fn hovered(&self) -> Option<ItemRef<'_, Self::Item>> {
        self.matches().get(self.hovered_rank()? as usize)
    }

    fn active_components(&self) -> Components {
        self.session().active_components
    }

    fn match_status(&self) -> MatchStatus {
        self.session().status
    }

    fn queue_status(&self) -> Option<QueueStatus> {
        let session = self.session();
        session.interface.queue_status(&session.state)
    }
}
