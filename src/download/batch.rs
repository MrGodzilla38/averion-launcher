use super::{
    ArtifactStatus, Cancellation, Download, Error, Failure, OverallProgress, Progress, Reporter,
    Stage, transfer,
};
use futures::{FutureExt, StreamExt, future::Either, stream::FuturesUnordered};
use std::{
    future::ready,
    sync::Mutex,
    time::{Duration, Instant},
};

const PARALLEL_FILES: usize = 8;

struct Totals {
    progress: OverallProgress,
    last_report: Option<Instant>,
}

impl Totals {
    fn update(&mut self, bytes: u64, complete: bool) -> Option<OverallProgress> {
        self.progress.bytes += bytes;
        self.progress.files += usize::from(complete);
        if self.progress.files == self.progress.total_files
            || self
                .last_report
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(100))
        {
            self.last_report = Some(Instant::now());
            Some(self.progress)
        } else {
            None
        }
    }
}

/// The caller supplies distinct destinations and optionally an already verified first file.
pub(crate) async fn run_verified(
    plan: &[Download],
    completed: Option<u64>,
    cancel: &Cancellation,
    report: &Reporter,
) -> Result<OverallProgress, Error> {
    let completed_files = usize::from(completed.is_some());
    let completed_bytes = completed.unwrap_or_default();
    let total_bytes = plan
        .iter()
        .try_fold(completed_bytes, |sum, file| {
            file.checksum.as_ref()?;
            sum.checked_add(file.expected_size?)
        })
        .ok_or_else(|| Error {
            component: "Minecraft files".into(),
            failure: Failure::InvalidPlan,
        })?;
    let totals = Mutex::new(Totals {
        progress: OverallProgress {
            bytes: completed_bytes,
            total_bytes,
            files: completed_files,
            total_files: plan.len() + completed_files,
        },
        last_report: None,
    });
    let mut files = plan.iter().enumerate();
    let mut work = FuturesUnordered::new();
    for (step, file) in files.by_ref().take(PARALLEL_FILES) {
        work.push(transfer_file(file, step + completed_files + 1, &totals, cancel, report).boxed());
    }
    while let Some(result) = work.next().await {
        result?;
        if let Some((step, file)) = files.next() {
            work.push(
                transfer_file(file, step + completed_files + 1, &totals, cancel, report).boxed(),
            );
        }
    }
    Ok(totals.lock().unwrap().progress)
}

async fn transfer_file(
    file: &Download,
    step: usize,
    totals: &Mutex<Totals>,
    cancel: &Cancellation,
    report: &Reporter,
) -> Result<(), Error> {
    let error = |failure| Error {
        component: file.name.clone(),
        failure,
    };
    let mut progress = Progress {
        component: file.name.clone(),
        step,
        count: totals.lock().unwrap().progress.total_files,
        stage: Stage::Checking,
        overall: None,
    };
    let mut received = 0;
    let mut update = |mut next: Progress| {
        let delta = match next.stage {
            Stage::Receiving {
                received: current, ..
            } => {
                let delta = current - received;
                received = current;
                delta
            }
            _ => 0,
        };
        next.overall = totals.lock().unwrap().update(delta, false);
        if next.overall.is_some() {
            Either::Left(report(next))
        } else {
            Either::Right(ready(()))
        }
    };
    let mut buffer = vec![0; 64 * 1024];
    let result = transfer(file, cancel, &mut buffer, &mut progress, &mut update)
        .await
        .map_err(error)?;
    if result == ArtifactStatus::Transferred {
        return Err(error(Failure::InvalidPlan));
    }

    // Cached files are counted once verified. Network files have reported their final byte count.
    let size = file
        .expected_size
        .ok_or_else(|| error(Failure::InvalidPlan))?;
    progress.overall = totals.lock().unwrap().update(size - received, true);
    if progress.overall.is_some() {
        cancel.wait(report(progress)).await.map_err(error)?;
    }
    Ok(())
}
