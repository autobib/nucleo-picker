use std::{
    collections::BTreeMap,
    num::NonZero,
    sync::{Arc, Mutex},
};

use tracing::{
    Dispatch, Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

use super::FrameState;
use crate::{
    PickerOptions,
    match_list::{Queued, SelectedIndices},
    render::DisplayRenderer,
};

#[derive(Default)]
struct Recorded {
    name: String,
    target: String,
    fields: BTreeMap<String, String>,
}

struct Recorder(Arc<Mutex<Recorded>>);

impl Subscriber for Recorder {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "nucleo_picker::frame"
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut recorded = self.0.lock().unwrap();
        recorded.name = attributes.metadata().name().to_owned();
        recorded.target = attributes.metadata().target().to_owned();
        attributes.record(&mut FieldVisitor(&mut recorded.fields));
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut recorded = self.0.lock().unwrap();
        values.record(&mut FieldVisitor(&mut recorded.fields));
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}

    fn event(&self, _: &Event<'_>) {}
}

struct FieldVisitor<'a>(&'a mut BTreeMap<String, String>);

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }
}

#[test]
fn frame_span_has_semantic_fields() {
    let recorded = Arc::new(Mutex::new(Recorded::default()));
    let dispatch = Dispatch::new(Recorder(recorded.clone()));
    let picker = PickerOptions::default()
        .query("needle")
        .reversed(true)
        .max_selection_count(NonZero::new(3))
        .picker::<String, _>(DisplayRenderer);
    let mut queued = SelectedIndices::init(NonZero::new(3));
    assert!(queued.toggle(42));
    let frame = FrameState {
        frame: 7,
        width: 80,
        height: 24,
        matching: true,
        injecting: true,
        ..FrameState::default()
    };

    tracing::dispatcher::with_default(&dispatch, || {
        let span = frame.trace_span(picker.max_selection_count, picker.reversed);
        frame.record_trace(&span, &picker, &queued);
    });

    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.name, "picker.frame");
    assert_eq!(recorded.target, "nucleo_picker::frame");
    assert_eq!(recorded.fields["sequence"], "7");
    assert_eq!(recorded.fields["width"], "80");
    assert_eq!(recorded.fields["height"], "24");
    assert_eq!(recorded.fields["query"], "needle");
    assert_eq!(recorded.fields["matching"], "true");
    assert_eq!(recorded.fields["injecting"], "true");
    assert_eq!(recorded.fields["matched"], "0");
    assert_eq!(recorded.fields["total"], "0");
    assert_eq!(recorded.fields["queued"], "1");
    assert_eq!(recorded.fields["selection_limit"], "3");
    assert_eq!(recorded.fields["reversed"], "true");
    assert!(!recorded.fields.contains_key("selected_match"));
    assert!(!recorded.fields.contains_key("selected_input_index"));
}
