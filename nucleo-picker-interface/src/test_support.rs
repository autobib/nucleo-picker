use nucleo_picker_core::{
    Render,
    event::MatchListEvent,
    session::{Components, Context, Exit, Model},
};

use crate::Interface;

struct Capture;

struct Captured<'items, T>(Context<'items, T>);

impl<'items, T> IntoIterator for Captured<'items, T> {
    type Item = &'items T;
    type IntoIter = std::iter::Empty<&'items T>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::empty()
    }
}

impl<T: Send + Sync + 'static> Interface<T> for Capture {
    type State = ();
    type Output<'items> = Captured<'items, T>;

    const PREVIEW: bool = true;

    fn init(&mut self, _: &Context<'_, T>) {}

    fn finish<'items>(&mut self, (): (), _: Exit, cx: Context<'items, T>) -> Self::Output<'items> {
        Captured(cx)
    }
}

pub(crate) fn context<T: Send + Sync + 'static, R: Render<T>>(
    model: &mut Model<T, R>,
    rank: Option<u32>,
) -> Context<'_, T> {
    context_with_components(model, rank, Components::ALL)
}

pub(crate) fn context_with_components<T: Send + Sync + 'static, R: Render<T>>(
    model: &mut Model<T, R>,
    rank: Option<u32>,
    components: Components,
) -> Context<'_, T> {
    let mut session = model.session(Capture);
    {
        let mut events = session.events();
        events.match_list(MatchListEvent::Reset);
        if let Some(rank) = rank {
            events.match_list(MatchListEvent::Up(rank as usize));
        }
        events.set_active_components(components);
    }
    let cx = session.finish(Exit::Quit).0;
    if let Some(rank) = rank {
        assert_eq!(cx.hovered_rank(), Some(rank));
    }
    assert_eq!(cx.active_components(), components);
    cx
}
