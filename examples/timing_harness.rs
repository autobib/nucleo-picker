//! # Manual picker timing harness
//!
//! Populates a picker with synthetic data, accepts terminal input for five seconds, and writes
//! per-frame latency measurements to `timing.json`.
use std::{
    collections::HashMap,
    convert::Infallible,
    fs::File,
    io::{self, BufWriter},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use nucleo_picker::{
    PickerOptions,
    event::{Event, EventSource, RecvError, StdinReader},
    render::StrRenderer,
};
use rand::{RngExt, SeedableRng, rngs::StdRng};
use serde::Serialize;
use tracing::{Id, Subscriber, field::Visit, span::Attributes};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

const ITEM_COUNT: usize = 500_000;
const RUN_TIME: Duration = Duration::from_secs(5);
const ITEM_SEED: u64 = 0x6e75_636c_656f_0001;

#[derive(Clone, Default, Serialize)]
struct FrameTiming {
    frame: usize,
    total_ns: u64,
    input_wait_ns: u64,
    semantic_event_handling_ns: u64,
    buffered_event_application_ns: u64,
    matcher_update_ns: u64,
    frame_rendering_ns: u64,
    waits: Vec<WaitTiming>,
}

#[derive(Clone, Default, Serialize)]
struct WaitTiming {
    requested_timeout_us: Option<u64>,
    reported_elapsed_us: Option<u64>,
    measured_elapsed_ns: u64,
    result: Option<String>,
}

#[derive(Serialize)]
struct TimingReport {
    item_count: usize,
    run_time_ms: u64,
    item_seed: u64,
    frames: Vec<FrameTiming>,
}

#[derive(Default)]
struct SpanFields {
    timeout_us: Option<u64>,
    elapsed_us: Option<u64>,
    result: Option<String>,
}

impl Visit for SpanFields {
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "timeout_us" => self.timeout_us = Some(value),
            "elapsed_us" => self.elapsed_us = Some(value),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "result" {
            self.result = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
}

struct SpanState {
    name: &'static str,
    started: Option<Instant>,
    fields: SpanFields,
}

#[derive(Default)]
struct TimingState {
    spans: HashMap<Id, SpanState>,
    frames: Vec<FrameTiming>,
    active_frame: Option<usize>,
}

#[derive(Clone, Default)]
struct FrameTimingLayer(Arc<Mutex<TimingState>>);

impl<S: Subscriber> Layer<S> for FrameTimingLayer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _ctx: Context<'_, S>) {
        let mut fields = SpanFields::default();
        attrs.record(&mut fields);
        self.0.lock().unwrap().spans.insert(
            id.clone(),
            SpanState {
                name: attrs.metadata().name(),
                started: None,
                fields,
            },
        );
    }

    fn on_record(&self, id: &Id, values: &tracing::span::Record<'_>, _ctx: Context<'_, S>) {
        if let Some(span) = self.0.lock().unwrap().spans.get_mut(id) {
            values.record(&mut span.fields);
        }
    }

    fn on_enter(&self, id: &Id, _ctx: Context<'_, S>) {
        let mut state = self.0.lock().unwrap();
        let Some(name) = state.spans.get(id).map(|span| span.name) else {
            return;
        };
        if name == "picker.frame" {
            let frame = state.frames.len();
            state.frames.push(FrameTiming {
                frame,
                ..FrameTiming::default()
            });
            state.active_frame = Some(frame);
        }
        if let Some(span) = state.spans.get_mut(id) {
            span.started = Some(Instant::now());
        }
    }

    fn on_exit(&self, id: &Id, _ctx: Context<'_, S>) {
        let mut state = self.0.lock().unwrap();
        let Some(span) = state.spans.get_mut(id) else {
            return;
        };
        let Some(started) = span.started.take() else {
            return;
        };
        let elapsed_ns = nanos(started.elapsed());
        let name = span.name;
        let wait = (name == "picker.event.wait").then(|| WaitTiming {
            requested_timeout_us: span.fields.timeout_us,
            reported_elapsed_us: span.fields.elapsed_us,
            measured_elapsed_ns: elapsed_ns,
            result: span.fields.result.clone(),
        });
        let Some(frame_index) = state.active_frame else {
            return;
        };
        let frame = &mut state.frames[frame_index];
        match name {
            "picker.frame" => {
                frame.total_ns = elapsed_ns;
                state.active_frame = None;
            }
            "picker.event.wait" => {
                frame.input_wait_ns += elapsed_ns;
                frame.waits.push(wait.unwrap());
            }
            "picker.event.handle" => frame.semantic_event_handling_ns += elapsed_ns,
            "picker.events.apply" => frame.buffered_event_application_ns += elapsed_ns,
            "picker.match.tick" => frame.matcher_update_ns += elapsed_ns,
            "picker.frame.render" => frame.frame_rendering_ns += elapsed_ns,
            _ => {}
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        self.0.lock().unwrap().spans.remove(&id);
    }
}

fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

struct TimedStdin {
    stdin: StdinReader,
    finish: Instant,
}

impl TimedStdin {
    fn new() -> Self {
        Self {
            stdin: StdinReader::default(),
            finish: Instant::now() + RUN_TIME,
        }
    }
}

impl EventSource for TimedStdin {
    type AbortErr = Infallible;

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Event, RecvError> {
        let remaining = self.finish.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(Event::Quit);
        }
        match self.stdin.recv_timeout(timeout.min(remaining)) {
            Err(RecvError::Timeout) if Instant::now() >= self.finish => Ok(Event::Quit),
            result => result,
        }
    }
}

fn random_item(rng: &mut StdRng) -> String {
    let len = rng.random_range(48..=80);
    (0..len)
        .map(|_| {
            if rng.random_range(0..12) == 0 {
                ' '
            } else {
                rng.random_range('a'..='z')
            }
        })
        .collect()
}

fn main() -> io::Result<()> {
    let timing = FrameTimingLayer::default();
    tracing_subscriber::registry().with(timing.clone()).init();

    let mut picker = PickerOptions::default().picker(StrRenderer);
    let injector = picker.injector();
    thread::spawn(move || {
        let mut rng = StdRng::seed_from_u64(ITEM_SEED);
        injector.push_batch((0..ITEM_COUNT).map(|_| random_item(&mut rng)));
    });

    eprintln!(
        "running for {} seconds with {ITEM_COUNT} generated items; type and scroll now",
        RUN_TIME.as_secs()
    );
    let mut terminal = BufWriter::new(io::stderr());
    let selection = picker
        .pick_with_io(TimedStdin::new(), &mut terminal)
        .map_err(io::Error::other)?;
    assert!(selection.is_none());

    let state = timing.0.lock().unwrap();
    let report = TimingReport {
        item_count: ITEM_COUNT,
        run_time_ms: RUN_TIME.as_millis() as u64,
        item_seed: ITEM_SEED,
        frames: state.frames.clone(),
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create("timing.json")?), &report)?;
    eprintln!("wrote {} frames to timing.json", report.frames.len());
    Ok(())
}
