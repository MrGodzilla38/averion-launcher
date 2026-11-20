use super::*;
use futures::{FutureExt, executor::block_on, future, io::Cursor};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

const PAYLOAD: &[u8] = b"abc";

fn checksum() -> Checksum {
    Checksum::Sha256([
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ])
}

fn job(
    dir: &Path,
    name: &str,
    open: impl Fn() -> BoxFuture<'static, io::Result<Response>> + Send + Sync + 'static,
) -> Download {
    Download {
        name: name.into(),
        destination: dir.join(name),
        expected_size: Some(3),
        checksum: Some(checksum()),
        existing: crate::download::ExistingFile::ReplaceInvalid,
        open: Box::new(open),
    }
}

fn response(body: impl AsyncRead + Unpin + Send + 'static, total: Option<u64>) -> Response {
    Response {
        body: Box::new(body),
        total,
    }
}

fn execute(plan: &[Download]) -> (Result<Vec<ArtifactStatus>, Error>, Vec<Progress>) {
    let (_sender, cancel) = cancellation();
    let mut events = Vec::new();
    let result = block_on(run(plan, cancel, |event| {
        events.push(event);
        future::ready(())
    }));
    (result, events)
}

fn assert_no_part_files(dir: &Path) {
    let entries = fs::read_dir(dir).unwrap();
    assert!(
        entries
            .map(|entry| entry.unwrap().path())
            .all(|path| !path.extension().is_some_and(|ext| ext == "part"))
    );
}

enum Tail {
    Eof,
    Interrupted,
    Pending,
}

struct Chunked {
    head: &'static [u8],
    tail: Tail,
}

impl AsyncRead for Chunked {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if !self.head.is_empty() {
            let count = buffer.len().min(self.head.len());
            buffer[..count].copy_from_slice(&self.head[..count]);
            self.head = &self.head[count..];
            return Poll::Ready(Ok(count));
        }
        match self.tail {
            Tail::Eof => Poll::Ready(Ok(0)),
            Tail::Interrupted => Poll::Ready(Err(io::ErrorKind::ConnectionReset.into())),
            Tail::Pending => Poll::Pending,
        }
    }
}

#[test]
fn reports_actual_bytes_and_components_and_reuses_verified_files() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let first = job(dir.path(), "first", {
        let calls = calls.clone();
        move || {
            calls.fetch_add(1, Ordering::Relaxed);
            async {
                let split = Cursor::new(&PAYLOAD[..1]).chain(Cursor::new(&PAYLOAD[1..]));
                Ok(response(split, Some(3)))
            }
            .boxed()
        }
    });
    let mut second = job(dir.path(), "second", {
        let calls = calls.clone();
        move || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Ok(response(Cursor::new(PAYLOAD), None)) }.boxed()
        }
    });
    second.expected_size = None;
    let plan = [first, second];
    let (result, events) = execute(&plan);
    assert_eq!(result.unwrap(), [ArtifactStatus::Verified; 2]);
    let stages: Vec<_> = events
        .iter()
        .map(|event| (event.component.as_ref(), event.step, &event.stage))
        .collect();
    use Stage::{Checking, Receiving};
    assert_eq!(
        stages,
        [
            ("first", 1, &Checking),
            (
                "first",
                1,
                &Receiving {
                    received: 0,
                    total: Some(3)
                }
            ),
            (
                "first",
                1,
                &Receiving {
                    received: 1,
                    total: Some(3)
                }
            ),
            (
                "first",
                1,
                &Receiving {
                    received: 3,
                    total: Some(3)
                }
            ),
            ("second", 2, &Checking),
            (
                "second",
                2,
                &Receiving {
                    received: 0,
                    total: None
                }
            ),
            (
                "second",
                2,
                &Receiving {
                    received: 3,
                    total: None
                }
            ),
        ]
    );
    assert!(events.iter().all(|event| event.count == 2));
    assert!((events[2].percent().unwrap() - 100. / 3.).abs() < 0.001);
    assert_eq!(events[3].percent(), Some(100.));
    assert_eq!(events[6].percent(), None);
    for task in &plan {
        assert_eq!(fs::read(&task.destination).unwrap(), PAYLOAD);
    }
    assert_eq!(execute(&plan).0.unwrap(), [ArtifactStatus::Reused; 2]);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_no_part_files(dir.path());
}

#[test]
fn cancellation_wakes_a_pending_request() {
    let dir = tempfile::tempdir().unwrap();
    let plan = [job(dir.path(), "waiting", || future::pending().boxed())];
    let (sender, cancel) = cancellation();
    let mut transfer = pin!(run(&plan, cancel, async |_| {}));
    assert!(transfer.as_mut().now_or_never().is_none());
    sender.send(()).unwrap();
    assert_eq!(block_on(transfer).unwrap_err().failure, Failure::Cancelled);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn cancellation_wakes_a_stalled_body_and_preserves_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let first = job(dir.path(), "existing", || {
        async {
            Ok(response(
                Chunked {
                    head: b"a",
                    tail: Tail::Pending,
                },
                None,
            ))
        }
        .boxed()
    });
    fs::write(&first.destination, b"old").unwrap();
    let next = job(dir.path(), "never", || panic!("cancelled plan advanced"));
    let plan = [first, next];
    let (sender, cancel) = cancellation();
    let mut transfer = pin!(run(&plan, cancel, async |_| {}));

    assert!(transfer.as_mut().now_or_never().is_none());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    sender.send(()).unwrap();
    assert_eq!(block_on(transfer).unwrap_err().failure, Failure::Cancelled);
    assert_eq!(fs::read(&plan[0].destination).unwrap(), b"old");
    assert!(!plan[1].destination.exists());
    assert_no_part_files(dir.path());
}

#[test]
fn interruption_cleans_staging_and_retry_reuses_committed_components() {
    let dir = tempfile::tempdir().unwrap();
    let first = job(dir.path(), "first", || {
        async { Ok(response(Cursor::new(PAYLOAD), Some(3))) }.boxed()
    });
    let second = job(dir.path(), "second", {
        let calls = AtomicUsize::new(0);
        move || {
            let call = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                if call == 0 {
                    Ok(response(
                        Chunked {
                            head: b"a",
                            tail: Tail::Interrupted,
                        },
                        Some(3),
                    ))
                } else {
                    Ok(response(Cursor::new(PAYLOAD), Some(3)))
                }
            }
            .boxed()
        }
    });
    fs::write(&second.destination, b"old").unwrap();
    let plan = [first, second];
    assert_eq!(
        execute(&plan).0.unwrap_err(),
        Error {
            component: "second".into(),
            failure: Failure::Network(io::ErrorKind::ConnectionReset)
        }
    );
    assert_eq!(fs::read(&plan[0].destination).unwrap(), PAYLOAD);
    assert_eq!(fs::read(&plan[1].destination).unwrap(), b"old");
    assert_no_part_files(dir.path());
    assert_eq!(
        execute(&plan).0.unwrap(),
        [ArtifactStatus::Reused, ArtifactStatus::Verified]
    );
    assert_eq!(fs::read(&plan[1].destination).unwrap(), PAYLOAD);
    assert_no_part_files(dir.path());
}

#[test]
fn rejects_truncated_oversized_and_wrong_digest_bodies() {
    for (body, advertised, expected) in [
        (&b"ab"[..], Some(3), Failure::SizeMismatch),
        (&b"abcd"[..], None, Failure::SizeMismatch),
        (&b"bad"[..], Some(3), Failure::ChecksumMismatch),
        (PAYLOAD, Some(2), Failure::SizeMismatch),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let plan = [job(dir.path(), "bad", move || {
            async move {
                Ok(response(
                    Chunked {
                        head: body,
                        tail: Tail::Eof,
                    },
                    advertised,
                ))
            }
            .boxed()
        })];
        assert_eq!(execute(&plan).0.unwrap_err().failure, expected);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn complete_transfer_without_a_checksum_is_not_verified_or_reused() {
    let dir = tempfile::tempdir().unwrap();
    let mut download = job(dir.path(), "artifact", || {
        async { Ok(response(Cursor::new(PAYLOAD), Some(3))) }.boxed()
    });
    download.checksum = None;
    fs::write(&download.destination, b"old").unwrap();
    let plan = [download];

    for _ in 0..2 {
        assert_eq!(execute(&plan).0.unwrap(), [ArtifactStatus::Transferred]);
        assert_eq!(fs::read(&plan[0].destination).unwrap(), PAYLOAD);
        assert_no_part_files(dir.path());
    }
}

#[test]
fn abandoning_a_pending_transfer_drops_its_staging_file() {
    let dir = tempfile::tempdir().unwrap();
    let plan = [job(dir.path(), "pending", || {
        async {
            Ok(response(
                Chunked {
                    head: b"a",
                    tail: Tail::Pending,
                },
                None,
            ))
        }
        .boxed()
    })];
    let (_sender, cancel) = cancellation();

    {
        let mut transfer = pin!(run(&plan, cancel, async |_| {}));
        assert!(transfer.as_mut().now_or_never().is_none());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn cancellation_at_full_byte_count_prevents_publication() {
    let dir = tempfile::tempdir().unwrap();
    let plan = [job(dir.path(), "existing", || {
        async { Ok(response(Cursor::new(PAYLOAD), Some(3))) }.boxed()
    })];
    fs::write(&plan[0].destination, b"old").unwrap();
    let (sender, cancel) = cancellation();
    let mut sender = Some(sender);
    let result = block_on(run(&plan, cancel, |progress| {
        if progress.percent() == Some(100.) {
            sender.take().unwrap().send(()).unwrap();
        }
        future::ready(())
    }));
    assert_eq!(result.unwrap_err().failure, Failure::Cancelled);
    assert_eq!(fs::read(&plan[0].destination).unwrap(), b"old");
    assert_no_part_files(dir.path());
}

#[test]
fn preserve_policy_keeps_a_file_created_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    let mut download = job(dir.path(), "artifact", || {
        async { Ok(response(Cursor::new(PAYLOAD), Some(3))) }.boxed()
    });
    download.existing = ExistingFile::Preserve;
    let plan = [download];
    let (_sender, cancel) = cancellation();
    let result = block_on(run(&plan, cancel, |progress| {
        if progress.percent() == Some(100.) {
            fs::write(&plan[0].destination, b"user data").unwrap();
        }
        future::ready(())
    }));
    assert_eq!(result.unwrap_err().failure, Failure::ExistingFile);
    assert_eq!(fs::read(&plan[0].destination).unwrap(), b"user data");
    assert_no_part_files(dir.path());
}

#[test]
fn bounded_workers_stop_on_cancellation_and_retry_reuses_verified_files() {
    use std::sync::{Mutex, atomic::AtomicBool};

    let dir = tempfile::tempdir().unwrap();
    let retry = Arc::new(AtomicBool::new(false));
    let opens = Arc::new(AtomicUsize::new(0));
    let plan: Vec<_> = (0..12)
        .map(|index| {
            job(dir.path(), &index.to_string(), {
                let retry = retry.clone();
                let opens = opens.clone();
                move || {
                    opens.fetch_add(1, Ordering::Relaxed);
                    let retry = retry.load(Ordering::Relaxed);
                    async move {
                        let tail = if retry { Tail::Eof } else { Tail::Pending };
                        Ok(response(
                            Chunked {
                                head: PAYLOAD,
                                tail,
                            },
                            Some(3),
                        ))
                    }
                    .boxed()
                }
            })
        })
        .collect();
    fs::write(&plan[0].destination, PAYLOAD).unwrap();

    let (sender, cancel) = cancellation();
    let report = |_: Progress| future::ready(()).boxed();
    let mut work = pin!(run_verified(&plan, Some(7), &cancel, &report));
    assert!(work.as_mut().now_or_never().is_none());
    assert_eq!(opens.load(Ordering::Relaxed), 8);
    sender.send(()).unwrap();
    assert_eq!(block_on(work).unwrap_err().failure, Failure::Cancelled);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_no_part_files(dir.path());

    retry.store(true, Ordering::Relaxed);
    let (_sender, cancel) = cancellation();
    let updates = Arc::new(Mutex::new(Vec::new()));
    let report = {
        let updates = updates.clone();
        move |progress: Progress| {
            updates.lock().unwrap().push(progress.overall.unwrap());
            future::ready(()).boxed()
        }
    };
    let overall = block_on(run_verified(&plan, Some(7), &cancel, &report)).unwrap();
    assert_eq!(opens.load(Ordering::Relaxed), 8 + plan.len() - 1);
    assert_eq!(
        overall,
        OverallProgress {
            bytes: 43,
            total_bytes: 43,
            files: 13,
            total_files: 13
        }
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), plan.len());
    let updates = updates.lock().unwrap();
    assert_eq!(updates.last(), Some(&overall));
    assert!(
        updates
            .windows(2)
            .all(|window| window[0].bytes <= window[1].bytes)
    );
}
