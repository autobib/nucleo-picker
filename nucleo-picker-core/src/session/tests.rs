use std::{
    cell::Cell,
    thread,
    time::{Duration, Instant},
};

use crate::{
    Render,
    interface::Interface,
    render::StrRenderer,
    session::{Context, Exit, FrameOptions, ItemId, Model, ModelOptions, QueueStatus, Session},
};

pub(crate) use crate::component::tests::TestSurface;

mod context;
mod selection;
mod session;
mod updates;

pub(crate) struct Select;

impl<T: Send + Sync + 'static> Interface<T> for Select {
    type State = ();
    type Output<'items> = Option<&'items T>;

    fn init(&mut self, _: &Context<'_, T>) {}

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        match exit {
            Exit::Quit | Exit::Selected(None) => None,
            Exit::Selected(Some(id)) => cx.matches().item(id).map(|item| item.value()),
        }
    }
}

pub(crate) struct Queued<'a>(pub(crate) &'a Cell<Option<ItemId>>);

impl<T: Send + Sync + 'static> Interface<T> for Queued<'_> {
    type State = ();
    type Output<'items> = Option<&'items T>;

    fn init(&mut self, _: &Context<'_, T>) {}

    fn is_queued(&self, _: &(), id: ItemId) -> bool {
        self.0.get() == Some(id)
    }

    fn queue_status(&self, _: &()) -> Option<QueueStatus> {
        Some(QueueStatus {
            count: u32::from(self.0.get().is_some()),
            limit: None,
        })
    }

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        Select.finish((), exit, cx)
    }
}

pub(crate) fn frame_options() -> FrameOptions {
    FrameOptions::new(Instant::now())
}

pub(crate) fn update<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>>(
    session: &mut Session<'_, T, R, I>,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session
        .update_matches(Duration::from_millis(5))
        .is_complete()
    {
        assert!(Instant::now() < deadline, "matching did not complete");
        thread::yield_now();
    }
}

fn model(items: impl IntoIterator<Item = &'static str>) -> Model<String, StrRenderer> {
    let mut model = ModelOptions::new().sort_results(false).model(StrRenderer);
    model.push_batch(items.into_iter().map(str::to_owned));
    model.filter().unwrap();
    model
}
