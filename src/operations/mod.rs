// OpenStreetMap operation implementations.
//
// Each module wraps one upstream OSM service call and normalizes its response
// into a stable JSON shape for the MCP tool layer.

pub mod lookup;
pub mod nearby;
pub mod nominatim;
pub mod reverse;
pub mod route;
pub mod search;

/// A minimal capturing `tracing` layer, shared by the outbound-request
/// `debug!` unit tests in each operation module (mcp-core#40). Every module
/// logs one event right before its outbound call, in the same shape, so one
/// small capture helper here replaces five hand-rolled copies (rule 7.3).
#[cfg(test)]
pub(crate) mod test_capture {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt};

    /// One event, as the subscriber saw it.
    #[derive(Clone, Debug)]
    pub(crate) struct LoggedEvent {
        pub level: tracing::Level,
        pub fields: BTreeMap<String, String>,
    }

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<LoggedEvent>>>);

    struct Collector<'a>(&'a mut BTreeMap<String, String>);

    impl Visit for Collector<'_> {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name().to_string(), value.to_string());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }

    impl<S: tracing::Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut fields = BTreeMap::new();
            event.record(&mut Collector(&mut fields));
            self.0
                .lock()
                .expect("capture lock is only held to push one record")
                .push(LoggedEvent {
                    level: *event.metadata().level(),
                    fields,
                });
        }
    }

    /// Run `body` with a capturing subscriber installed, and return every
    /// event it logged.
    pub(crate) fn capture_events(body: impl FnOnce()) -> Vec<LoggedEvent> {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        tracing::subscriber::with_default(subscriber, body);
        capture
            .0
            .lock()
            .expect("capture lock is only held to push one record")
            .clone()
    }
}
