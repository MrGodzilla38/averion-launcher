use crate::{
    download::{self, Download, Error, Failure, Progress, Stage},
    mojang::{self, Mojang, Runtime},
};
use futures::{FutureExt, SinkExt, Stream, channel::oneshot};
use std::{fmt, sync::Arc};

#[derive(Clone, Debug)]
pub enum Event {
    Progress(Progress),
    Finished(Result<Option<Runtime>, JobError>),
}

#[derive(Clone, Debug)]
pub struct JobError {
    message: String,
    cancelled: bool,
}

impl From<Error> for JobError {
    fn from(error: Error) -> Self {
        Self {
            message: error.to_string(),
            cancelled: error.failure == Failure::Cancelled,
        }
    }
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Clone)]
pub(crate) enum Request {
    Prepared(Arc<[Download]>),
    Selected {
        version: &'static str,
        mojang: Mojang,
    },
}

#[derive(Default)]
pub(crate) enum Installation {
    #[default]
    Idle,
    Active {
        request: Request,
        cancel: Option<oneshot::Sender<()>>,
        progress: Progress,
    },
    Failed {
        request: Request,
        error: JobError,
    },
}

impl Installation {
    pub fn start(&mut self, plan: Arc<[Download]>) -> Option<impl Stream<Item = Event> + use<>> {
        self.start_request(Request::Prepared(plan))
    }

    pub fn start_selected(
        &mut self,
        version: &'static str,
        mojang: Mojang,
    ) -> Option<impl Stream<Item = Event> + use<>> {
        self.start_request(Request::Selected { version, mojang })
    }

    fn start_request(&mut self, request: Request) -> Option<impl Stream<Item = Event> + use<>> {
        if self.is_active() {
            return None;
        }
        let progress = match &request {
            Request::Prepared(plan) => Progress {
                component: plan.first()?.name.clone(),
                step: 1,
                count: plan.len(),
                stage: Stage::Checking,
                overall: None,
            },
            Request::Selected { version, .. } => Progress {
                component: format!("Minecraft {version}").into(),
                step: 0,
                count: 0,
                stage: Stage::Resolving,
                overall: None,
            },
        };
        let (sender, cancel) = download::cancellation();
        *self = Self::Active {
            request: request.clone(),
            cancel: Some(sender),
            progress,
        };
        Some(iced::stream::channel(4, async move |mut events| {
            let progress_sender = events.clone();
            let report = move |progress| {
                let mut events = progress_sender.clone();
                async move {
                    let _ = events.send(Event::Progress(progress)).await;
                }
                .boxed()
            };
            let result = match request {
                Request::Prepared(plan) => download::run(&plan, cancel, &report)
                    .await
                    .map(|_| None)
                    .map_err(JobError::from),
                Request::Selected { version, mojang } => mojang
                    .install(version, &cancel, &report)
                    .await
                    .map(Some)
                    .map_err(|error| match error {
                        mojang::Error::Download(error) => error.into(),
                        error => JobError {
                            message: format!("Minecraft {version} — {error}"),
                            cancelled: false,
                        },
                    }),
            };
            let _ = events.send(Event::Finished(result)).await;
        }))
    }

    pub fn retry(&mut self) -> Option<impl Stream<Item = Event> + use<>> {
        let Self::Failed { request, .. } = self else {
            return None;
        };
        let request = request.clone();
        self.start_request(request)
    }

    pub fn cancel(&mut self) {
        if let Self::Active { cancel, .. } = self
            && let Some(sender) = cancel.take()
        {
            let _ = sender.send(());
        }
    }

    pub fn update(&mut self, event: Event) -> Option<(&'static str, Runtime)> {
        let Self::Active {
            request, progress, ..
        } = self
        else {
            return None;
        };
        match event {
            Event::Progress(next) => *progress = next,
            Event::Finished(Err(error)) if !error.cancelled => {
                *self = Self::Failed {
                    request: request.clone(),
                    error,
                };
            }
            Event::Finished(Ok(runtime)) => {
                let completed = match request {
                    Request::Selected { version, .. } => runtime.map(|runtime| (*version, runtime)),
                    Request::Prepared(_) => None,
                };
                *self = Self::Idle;
                return completed;
            }
            Event::Finished(Err(_)) => *self = Self::Idle,
        }
        None
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    pub fn has_action(&self) -> bool {
        matches!(
            self,
            Self::Active {
                cancel: Some(_),
                ..
            } | Self::Failed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::Response;
    use futures::{FutureExt, StreamExt, executor::block_on, io::Cursor};
    use std::{
        fs, io,
        pin::pin,
        sync::atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn selected_release_resolution_can_be_cancelled_before_io() {
        block_on(async {
            let mut job = Installation::default();
            let mut events = pin!(job.start_selected("1.21", Mojang::default()).unwrap());
            assert!(matches!(&job, Installation::Active { progress, .. }
                if progress.component.as_ref() == "Minecraft 1.21" && progress.stage == Stage::Resolving));
            assert!(job.start_selected("1.20", Mojang::default()).is_none());
            job.cancel();
            let event = events.next().await.unwrap();
            assert!(matches!(&event, Event::Finished(Err(error)) if error.cancelled));
            let _ = job.update(event);
            assert!(matches!(job, Installation::Idle));
        });
    }

    #[test]
    fn selected_version_is_reported_complete_only_on_success() {
        let mut job = Installation::default();
        let _events = job.start_selected("1.20", Mojang::default()).unwrap();
        assert_eq!(
            job.update(Event::Finished(Err(JobError {
                message: "invalid native archive".into(),
                cancelled: false,
            }))),
            None
        );
        assert!(matches!(job, Installation::Failed { .. }));

        let _events = job.retry().unwrap();
        let runtime = Runtime {
            executable: "fixture/bin/java".into(),
            major: 17,
        };
        assert_eq!(
            job.update(Event::Finished(Ok(Some(runtime.clone())))),
            Some(("1.20", runtime))
        );
        assert!(matches!(job, Installation::Idle));
    }

    #[test]
    fn failed_stream_can_be_retried_until_it_finishes() {
        block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("component");
            let first_attempt = AtomicBool::new(true);
            let plan = Arc::from([Download {
                name: "Component".into(),
                destination: destination.clone(),
                expected_size: Some(3),
                checksum: None,
                existing: crate::download::ExistingFile::ReplaceInvalid,
                open: Box::new(move || {
                    let fail = first_attempt.swap(false, Ordering::Relaxed);
                    async move {
                        if fail {
                            Err(io::ErrorKind::ConnectionReset.into())
                        } else {
                            Ok(Response {
                                body: Box::new(Cursor::new(b"abc")),
                                total: Some(3),
                            })
                        }
                    }
                    .boxed()
                }),
            }]);
            let mut job = Installation::default();
            let mut events = pin!(job.start(plan).unwrap());
            while let Some(event) = events.next().await {
                let _ = job.update(event);
            }
            assert!(matches!(job, Installation::Failed { .. }));
            assert!(job.has_action());
            assert!(!destination.exists());

            let mut events = pin!(job.retry().unwrap());
            let mut saw_full_byte_count = false;
            while let Some(event) = events.next().await {
                let _ = job.update(event);
                if let Installation::Active { progress, .. } = &job {
                    saw_full_byte_count |= progress.percent() == Some(100.);
                }
            }
            assert!(saw_full_byte_count);
            assert!(matches!(job, Installation::Idle));
            assert!(!job.has_action());
            assert_eq!(fs::read(destination).unwrap(), b"abc");
        });
    }
}
