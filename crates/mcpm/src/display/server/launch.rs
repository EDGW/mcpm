//! Stderr launch stages and download progress; game streams remain owned by Java.

use std::time::{Duration, Instant};

use mcpm_backend::{
    download::{Event as DownloadEvent, RequestKind},
    local::mc::server::{
        authlib_injector::Event as AuthEvent,
        launch::{Event, Stage},
    },
};

use crate::{display::style::text, progress::Progress, terminal};

/// One active display, destroyed before Java starts or the shared diagnostic renderer runs.
#[derive(Default)]
pub(crate) struct LaunchProgress {
    progress: Option<Progress>,
    message: String,
    last_draw: Option<Instant>,
}

impl LaunchProgress {
    pub(crate) fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Stage(stage) => match stage {
                Stage::Configuration => self.phase("Reading launch configuration"),
                Stage::LocalChecks => self.phase("Checking Java and launch arguments"),
                Stage::JavaCapabilities => self.phase("Checking Java argument-file support"),
                Stage::Ready => self.phase("Launch preparation complete"),
            },
            Event::Authentication(event) => match event {
                AuthEvent::Checking => self.phase("Checking authlib-injector"),
                AuthEvent::Resolving => self.phase("Resolving authlib-injector release"),
                AuthEvent::Installing => self.phase("Installing authlib-injector"),
                AuthEvent::Ready => self.phase("authlib-injector ready"),
                AuthEvent::Download(event) => self.download(event),
            },
        }
    }

    /// Updates the final dynamic phase; the caller drops the display before spawning Java.
    pub(crate) fn starting(&mut self) {
        self.phase("Starting Java");
    }

    fn phase(&mut self, message: &str) {
        if !terminal::stderr_is_tty() || console::is_dumb() || self.message == message {
            return;
        }
        self.message = message.into();
        match &self.progress {
            Some(progress) => progress.phase(message),
            None => self.progress = Some(Progress::new(message, "Preparing launch")),
        }
    }

    fn download(&mut self, event: DownloadEvent<'_>) {
        match event {
            DownloadEvent::AttemptStarted { kind, source } => {
                let action = match kind {
                    RequestKind::Metadata => "Resolving authlib-injector release",
                    RequestKind::File => "Downloading authlib-injector",
                };
                self.phase(&format!("{action} · {}", text(source)));
                self.last_draw = None;
                if kind == RequestKind::File
                    && let Some(progress) = &self.progress
                {
                    progress.start_bytes(&self.message);
                }
            }
            DownloadEvent::Bytes { received, total } => {
                if self
                    .last_draw
                    .is_some_and(|last| last.elapsed() < Duration::from_millis(100))
                    && received != 0
                    && Some(received) != total
                {
                    return;
                }
                self.last_draw = Some(Instant::now());
                if let Some(progress) = &self.progress {
                    progress.bytes(received, total);
                }
            }
            DownloadEvent::Verifying => self.phase("Verifying authlib-injector download"),
            DownloadEvent::AttemptFailed { source, .. } => {
                self.phase(&format!("Download attempt failed · {}", text(source)));
            }
            DownloadEvent::Completed { .. } => {}
        }
    }
}
