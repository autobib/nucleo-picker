use super::super::*;
use crate::{Picker, PickerOptions, preview::request::QueuedPreviewRequest, render::StrRenderer};

#[derive(Default)]
pub(crate) struct TestPreviewer {
    pub(crate) focused: Vec<Option<&'static str>>,
    pub(crate) requested: Vec<&'static str>,
    pub(crate) buffer_allocations: Vec<*const u8>,
    pub(crate) queued: Vec<QueuedPreviewRequest<&'static str>>,
    pub(crate) defer: bool,
    pub(crate) drop_requests: bool,
    pub(crate) fail: bool,
    pub(crate) fail_after: Option<usize>,
    pub(crate) lines: usize,
    pub(crate) line_numbers: bool,
}

impl Preview<&'static str> for TestPreviewer {
    type AbortErr = &'static str;

    fn focus_changed(&mut self, item: Option<&&'static str>) {
        self.focused.push(item.copied());
    }

    fn preview(
        &mut self,
        item: &&'static str,
        request: PreviewRequest<'_, &'static str>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        assert!(timeout >= Duration::from_millis(2));
        assert!(std::ptr::eq(item, request.item()));
        assert_eq!(self.focused.last(), Some(&Some(*item)));
        self.requested.push(item);
        assert_eq!(request.buffer.lines().len(), 1);
        assert_eq!(request.buffer.line(0).unwrap().as_str(), "");
        assert!(!request.buffer.is_err());
        assert!(!request.buffer.line_numbers());
        self.buffer_allocations
            .push(request.buffer.line(0).unwrap().as_str().as_ptr());
        if self.fail
            || self
                .fail_after
                .is_some_and(|count| self.requested.len() > count)
        {
            return Err("preview failed");
        }
        if self.defer {
            let (pending, queued) = request.defer();
            if !self.drop_requests {
                self.queued.push(queued);
            }
            Ok(PreviewResponse::Pending(pending))
        } else {
            let mut buffer = request.ready();
            buffer.set_line_numbers(self.line_numbers);
            buffer.push_str(item);
            for _ in 1..self.lines {
                buffer.newline();
                buffer.push_str(item);
            }
            Ok(PreviewResponse::Ready(buffer))
        }
    }
}

pub(crate) fn settle(picker: &mut Picker<&'static str, StrRenderer>) {
    let start = std::time::Instant::now();
    loop {
        let status = picker.engine.update(5);
        if status.items_changed {
            picker
                .list_state
                .layout
                .update_items(picker.engine.snapshot(), &picker.list_state.config);
        }
        if !status.matching {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
}

pub(crate) fn picker(
    items: impl IntoIterator<Item = &'static str>,
) -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .picker(StrRenderer);
    picker.push_batch(items);
    settle(&mut picker);
    picker
        .list_state
        .layout
        .resize(picker.engine.snapshot(), 8, &picker.list_state.config);
    picker
}

pub(crate) fn selected_item(picker: &Picker<&'static str, StrRenderer>) -> Option<u32> {
    picker
        .list_state
        .layout
        .selection(picker.engine.snapshot())
        .map(|n| picker.engine.idx_from_match(n))
}

pub(crate) fn update(
    application: &mut PreviewPane<TestPreviewer>,
    picker: &mut Picker<&'static str, StrRenderer>,
) -> Result<bool, &'static str> {
    application.update(
        selected_item(picker),
        picker.engine.snapshot(),
        Instant::now(),
        true,
    )
}

#[cfg(feature = "unstable-backend")]
impl<P> PreviewPane<P> {
    pub(crate) fn test_cache(&mut self) -> &mut LruCache<u32, Cached> {
        &mut self.cache
    }

    pub(crate) fn test_previewer(&mut self) -> &mut P {
        &mut self.previewer
    }
}
