use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::super::testing::Started;
use super::*;

const ROW: StepId = StepId {
    kind: Kind::Hire,
    n: 99,
};

#[tokio::test(flavor = "multi_thread")]
async fn a_step_moves_only_from_the_last_one_done_of_an_open_row() {
    let started = Started::new().await;
    let commit = async |made: Change| started.commit(made).await;
    commit(Change::Begin { row: ROW }).await.unwrap();
    commit(Change::Advance {
        row: ROW,
        from: None,
        to: "a",
    })
    .await
    .unwrap();
    commit(Change::Advance {
        row: ROW,
        from: None,
        to: "b",
    })
    .await
    .unwrap_err();
    commit(Change::Close { row: ROW }).await.unwrap();
    commit(Change::Advance {
        row: ROW,
        from: Some("a"),
        to: "b",
    })
    .await
    .unwrap_err();

    let read = row(&started.rows(), ROW).await.unwrap().unwrap();
    assert_eq!(
        (read.done.as_deref(), read.closed),
        (Some("a"), true)
    );
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_row_begun_again_starts_over_once_it_has_closed() {
    let started = Started::new().await;
    let commit = async |made: Change| started.commit(made).await;
    let read = async || {
        let read = row(&started.rows(), ROW).await.unwrap().unwrap();
        (read.done, read.closed)
    };
    commit(Change::Begin { row: ROW }).await.unwrap();
    commit(Change::Advance {
        row: ROW,
        from: None,
        to: "a",
    })
    .await
    .unwrap();
    commit(Change::Begin { row: ROW }).await.unwrap();
    assert_eq!(read().await, (Some("a".to_owned()), false));

    commit(Change::Close { row: ROW }).await.unwrap();
    commit(Change::Begin { row: ROW }).await.unwrap();
    assert_eq!(read().await, (None, false));
    started.stop().await;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Two {
    First,
    Second,
}

impl Steps for Two {
    const ALL: &'static [Self] = &[Self::First, Self::Second];

    fn name(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Second => "second",
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_walk_runs_each_step_left_once_in_order() {
    let started = Started::new().await;
    let closed = StepId { n: 98, ..ROW };
    for row in [ROW, closed] {
        started.commit(Change::Begin { row }).await.unwrap();
    }
    started.commit(Change::Close { row: closed }).await.unwrap();
    let cx = Cx {
        realm: &started.realm,
        ctx: &started.ctx,
    };
    let ran = RefCell::new(Vec::new());
    let run = async |_: &Cx<'_>, step: Two| {
        ran.borrow_mut().push(step);
        Ok(Vec::new())
    };
    for row in [ROW, ROW, closed] {
        walk(&cx, row, run).await.unwrap();
    }

    assert_eq!(*ran.borrow(), [Two::First, Two::Second]);
    let rows = started.rows();
    let read = row(&rows, ROW).await.unwrap().unwrap();
    assert_eq!(
        (read.done.as_deref(), read.closed),
        (Some("second"), true)
    );
    let at_start = undone(&rows, &Drives::default()).await.unwrap();
    assert!(
        at_start.rows.iter().all(|&(row, _)| row != ROW),
        "a start drives no finished row"
    );
    started.stop().await;
}

#[test]
fn a_reservation_dropped_unstarted_gives_its_rows_back() {
    let drives = Drives::default();
    drop(drives.take(&[ROW]));
    assert_eq!(drives.take(&[ROW]).rows.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_for_no_drive_ends_at_once() {
    let started = Started::new().await;
    let held =
        change::Transaction::begin(&started.ctx).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        Drives::default().finished(
            ROW,
            Duration::from_secs(60),
            held.abandon(),
        ),
    )
    .await
    .expect("no drive runs, so the wait ends at once");
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_for_a_drive_ends_after_its_commit() {
    let started = Started::new().await;
    let drives = Drives::default();
    let held =
        change::Transaction::begin(&started.ctx).await.unwrap();
    let reserved = drives.take(&[ROW]);
    let committed = Cell::new(false);
    let drive = async {
        started.commit(Change::Close { row: ROW }).await.unwrap();
        committed.set(true);
        drop(reserved);
    };
    let wait = async {
        drives
            .finished(ROW, Duration::from_secs(60), held.abandon())
            .await;
        committed.get()
    };
    let ((), ended_after_commit) =
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(drive, wait)
        })
        .await
        .expect("the wait ends with the drive");
    assert!(ended_after_commit);
    started.stop().await;
}
