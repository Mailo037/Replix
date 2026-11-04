//! Optional media work: thumbnails, filmstrips, waveforms, and preview audio prepared
//! ahead of time.
//!
//! None of it is needed to record or save, and all of it decodes video or audio, so it runs
//! one job at a time in a single turn. Each job carries a `Demand` that says who needs it:
//! the library (made ahead of time), or the editor open in a given round. Work for the open
//! editor goes first, and work made ahead of time gives up its turn as soon as editor work
//! waits for it or the window is hidden, as a game is then likely running. Editor work
//! stops when its round ends: the editor closed, or the window was hidden.
//!
//! Locks, outermost first; a thread only ever takes them in this order:
//! 1. a clip's preparation lock (`editor::lock_for`), held while that clip is prepared;
//! 2. the media turn, held while one job runs;
//! 3. `library::META_LOCK`, held only for a sidecar's read, change and write;
//! 4. short-lived locks (`Pending`, `WAKE`, queues), never held while waiting on another.
//! Waiting for the turn or a wake-up never happens while holding `META_LOCK`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

/// Who needs a job: 0 for the library (ahead of time), n for the editor in round n.
/// Rounds only grow, and a job needed by both keeps the editor's, the stronger claim.
pub struct Demand {
    need: AtomicU64,
    /// Gives way to any other editor job waiting: the filmstrip, which the editor can do
    /// without for a moment, so waveforms are not kept waiting behind it.
    yields: bool,
}

pub const AHEAD: u64 = 0;

/// The error of a job that stopped because nobody needs it any more. Not a failure: the
/// editor closed or the window was hidden, and whoever needs it asks again.
#[derive(Debug)]
pub struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stopped")
    }
}

impl std::error::Error for Stopped {}

/// Whether an error is a job having stopped rather than failed.
pub fn is_stopped(e: &anyhow::Error) -> bool {
    e.is::<Stopped>()
}

/// Rings whenever work made ahead of time may have become free to run: the turn was
/// released, the editor stopped waiting, or Replix came back into use.
static WAKE: (Mutex<()>, parking_lot::Condvar) = (parking_lot::const_mutex(()), parking_lot::Condvar::new());

pub fn wake() {
    let _held = WAKE.0.lock();
    WAKE.1.notify_all();
}

/// Waits until work made ahead of time may run, without checking over and over; the
/// one-second limit only guards against a missed ring.
pub fn wait_until_ahead_may_run() {
    let ahead = Demand::new(AHEAD);
    let mut held = WAKE.0.lock();
    while !ahead.wanted() {
        WAKE.1.wait_for(&mut held, std::time::Duration::from_secs(1));
    }
}

/// Waits until a job that gave way may run again: true then, false once nobody needs it.
pub fn wait_until_wanted(demand: &Demand) -> bool {
    let mut held = WAKE.0.lock();
    loop {
        if demand.wanted() {
            return true;
        }
        if !demand.alive() {
            return false;
        }
        WAKE.1.wait_for(&mut held, std::time::Duration::from_secs(1));
    }
}

impl Demand {
    pub fn new(need: u64) -> Arc<Self> {
        Arc::new(Self { need: AtomicU64::new(need), yields: false })
    }

    /// For work that gives way to other editor work, and waits for it.
    pub fn yielding(need: u64) -> Arc<Self> {
        Arc::new(Self { need: AtomicU64::new(need), yields: true })
    }

    fn need(&self) -> u64 {
        self.need.load(Ordering::Relaxed)
    }

    /// Whether whoever asked for the job still needs it, now or later: an editor round that
    /// has not ended. Work made ahead of time is never needed later in that sense.
    pub fn alive(&self) -> bool {
        let need = self.need();
        need != AHEAD && STOPPED_ROUND.load(Ordering::Relaxed) < need
    }

    fn for_editor(&self) -> bool {
        self.need() != AHEAD
    }

    /// Whether the job should run, or go on running, right now.
    pub fn wanted(&self) -> bool {
        crate::page_visible()
            && match self.need() {
                AHEAD => crate::app_focused() && EDITOR_WAITING.load(Ordering::Relaxed) == 0,
                // Editor work pauses too while another program has the focus, a game on the
                // next monitor say, and goes on once Replix has it again. A glance away of a
                // second or two does not stop it: a stopped job starts over from the beginning.
                round => {
                    STOPPED_ROUND.load(Ordering::Relaxed) < round
                        && crate::focused_lately()
                        && (!self.yields || EDITOR_WAITING.load(Ordering::Relaxed) == 0)
                }
            }
    }
}

/// The newest editor round that ended; its jobs and those of earlier rounds stop, even
/// ones that had not started yet.
static STOPPED_ROUND: AtomicU64 = AtomicU64::new(0);

pub fn end_round(round: u64) {
    STOPPED_ROUND.fetch_max(round, Ordering::Relaxed);
    // Work of that round waiting for its turn gives up at once, not on its next check.
    wake();
}

/// Page loads so far. Each page counts its rounds in a range of its own, above every
/// earlier page's, so a request of an old page that arrives late can never pass for one
/// of the new page.
static PAGES: AtomicU64 = AtomicU64::new(0);
const PAGE_ROUNDS: u32 = 32;

/// A page load starts: every round of earlier pages ends, with its work, also rounds no
/// request has named yet, and the first round the new page may use comes back.
pub fn new_page() -> u64 {
    let first = (PAGES.fetch_add(1, Ordering::Relaxed) + 1) << PAGE_ROUNDS;
    end_round(first - 1);
    first
}

/// Tests that end rounds take turns: ending one ends every earlier one too.
#[cfg(test)]
pub fn test_rounds() -> (MutexGuard<'static, ()>, u64) {
    static ROUNDS: Mutex<()> = parking_lot::const_mutex(());
    let held = ROUNDS.lock();
    // Editor work runs only while Replix has the focus; tests start with it.
    crate::FOCUSED.store(true, Ordering::Relaxed);
    (held, STOPPED_ROUND.load(Ordering::Relaxed) + 1)
}

static TURN: Mutex<()> = parking_lot::const_mutex(());
/// Editor jobs waiting for the turn; work made ahead of time gives way to them.
static EDITOR_WAITING: AtomicUsize = AtomicUsize::new(0);

fn turn(demand: &Demand) -> MutexGuard<'static, ()> {
    if !demand.for_editor() {
        return TURN.lock();
    }
    EDITOR_WAITING.fetch_add(1, Ordering::Relaxed);
    let turn = TURN.lock();
    EDITOR_WAITING.fetch_sub(1, Ordering::Relaxed);
    wake();
    turn
}

/// Waits for something on the editor's behalf (a clip's lock, say): work made ahead of time
/// that holds it sees the editor waiting and stops.
pub fn wait_as_editor<T>(wait: impl FnOnce() -> T) -> T {
    EDITOR_WAITING.fetch_add(1, Ordering::Relaxed);
    let got = wait();
    EDITOR_WAITING.fetch_sub(1, Ordering::Relaxed);
    wake();
    got
}

pub enum Outcome<T> {
    Done(T),
    /// Its demand went away; nothing failed.
    Stopped,
    Failed(anyhow::Error),
}

impl<T> Outcome<T> {
    /// As a result, with a stop as the `Stopped` error.
    pub fn result(self) -> anyhow::Result<T> {
        match self {
            Outcome::Done(value) => Ok(value),
            Outcome::Stopped => Err(Stopped.into()),
            Outcome::Failed(e) => Err(e),
        }
    }
}

/// Runs `work` in the shared turn. `work` gets a check to call while it runs, and stops
/// when that says no.
pub fn run<T>(demand: &Demand, work: impl FnOnce(&(dyn Fn() -> bool + Sync)) -> anyhow::Result<T>) -> Outcome<T> {
    let outcome = run_in_turn(demand, work);
    wake();
    outcome
}

fn run_in_turn<T>(demand: &Demand, work: impl FnOnce(&(dyn Fn() -> bool + Sync)) -> anyhow::Result<T>) -> Outcome<T> {
    let _turn = turn(demand);
    let stopped = AtomicBool::new(false);
    let go = || {
        let wanted = demand.wanted();
        if !wanted {
            stopped.store(true, Ordering::Relaxed);
        }
        wanted
    };
    if !go() {
        return Outcome::Stopped;
    }
    match work(&go) {
        Ok(value) => Outcome::Done(value),
        Err(e) if stopped.load(Ordering::Relaxed) || is_stopped(&e) => Outcome::Stopped,
        Err(e) => Outcome::Failed(e),
    }
}

/// Like `run`, for work its editor still needs after a pause: when it had to give way (to
/// other editor work, or because Replix lost the focus), it waits and runs again, until it
/// is done or its round ends. `work` is started afresh each time.
pub fn run_needed<T>(demand: &Demand, work: impl Fn(&(dyn Fn() -> bool + Sync)) -> anyhow::Result<T>) -> Outcome<T> {
    loop {
        match run(demand, &work) {
            Outcome::Stopped if demand.alive() && wait_until_wanted(demand) => continue,
            other => return other,
        }
    }
}

/// Jobs under way by key, so a second request joins the first instead of repeating it.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<String, Arc<Demand>>>);

impl Pending {
    /// The demand to start a new job with, or `None` when one is under way; that one then
    /// carries this need too, so opening a clip keeps its half-made filmstrip going.
    pub fn claim(&self, key: &str, need: u64, yields: bool) -> Option<Arc<Demand>> {
        let mut jobs = self.0.lock();
        if let Some(demand) = jobs.get(key) {
            demand.need.fetch_max(need, Ordering::Relaxed);
            return None;
        }
        let demand = if yields { Demand::yielding(need) } else { Demand::new(need) };
        jobs.insert(key.to_string(), demand.clone());
        Some(demand)
    }

    /// After a job stopped: whether to run it again, because someone needs it again since.
    /// Decided under the same lock as `claim`, so a request either lands before this, and the
    /// job runs again, or after it, and starts a new job; it is never lost in between.
    pub fn stopped(&self, key: &str) -> bool {
        let mut jobs = self.0.lock();
        let again = jobs.get(key).is_some_and(|d| d.wanted() || d.alive());
        if !again {
            jobs.remove(key);
        }
        again
    }

    pub fn finish(&self, key: &str) {
        self.0.lock().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_during_a_stop_is_never_lost() {
        let (_rounds, round) = test_rounds();
        let pending = Pending::default();
        // A job made ahead of time is under way; the editor of `round` opens the clip.
        let demand = pending.claim("clip", AHEAD, false).expect("a new job");
        assert!(pending.claim("clip", round, false).is_none(), "joins the job under way");
        assert!(demand.for_editor());
        // The job stopped for some reason while it is still needed: it runs again.
        assert!(pending.stopped("clip"));
        // The editor closes; the job stops and is gone, so the next request starts anew.
        end_round(round);
        assert!(!demand.wanted());
        assert!(!pending.stopped("clip"));
        assert!(pending.claim("clip", round + 1, false).is_some());
        pending.finish("clip");
        assert!(pending.claim("clip", AHEAD, false).is_some());
    }

    /// Opening a clip while its warm-up holds the turn: the warm-up stops at once, and the
    /// editor's work runs next.
    #[test]
    fn work_ahead_of_time_gives_way_to_the_editor() {
        let (_rounds, round) = test_rounds();
        crate::FOCUSED.store(true, Ordering::Relaxed);
        let started = std::sync::Barrier::new(2);
        let editor_done = std::time::Instant::now();
        std::thread::scope(|s| {
            let ahead = s.spawn(|| {
                run(&Demand::new(AHEAD), |go| {
                    started.wait();
                    while go() {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    anyhow::bail!("stopped")
                }) as Outcome<()>
            });
            started.wait();
            let editor = run(&Demand::new(round), |_| Ok(()));
            assert!(matches!(editor, Outcome::Done(())));
            assert!(matches!(ahead.join().unwrap(), Outcome::Stopped));
        });
        assert!(editor_done.elapsed() < std::time::Duration::from_secs(2));
        crate::FOCUSED.store(false, Ordering::Relaxed);
    }

    /// Work made ahead of time that stopped for the editor waits for its ring, then runs.
    #[test]
    fn stopped_work_waits_for_the_editor_without_spinning() {
        let (_rounds, _) = test_rounds();
        crate::FOCUSED.store(true, Ordering::Relaxed);
        let waiting = std::sync::atomic::AtomicBool::new(true);
        std::thread::scope(|s| {
            let editor = s.spawn(|| {
                wait_as_editor(|| std::thread::sleep(std::time::Duration::from_millis(300)));
                waiting.store(false, Ordering::Relaxed);
            });
            std::thread::sleep(std::time::Duration::from_millis(50));
            let started = std::time::Instant::now();
            wait_until_ahead_may_run();
            // It returned because the editor finished, not before, and promptly after.
            assert!(!waiting.load(Ordering::Relaxed) || EDITOR_WAITING.load(Ordering::Relaxed) == 0);
            assert!(started.elapsed() < std::time::Duration::from_millis(900));
            editor.join().unwrap();
        });
        crate::FOCUSED.store(false, Ordering::Relaxed);
    }

    /// A reloaded page ends the rounds of the page before it, even one never ended there.
    #[test]
    fn a_new_page_ends_the_old_pages_rounds() {
        let (_rounds, round) = test_rounds();
        let old = Demand::new(round);
        assert!(STOPPED_ROUND.load(Ordering::Relaxed) < round);
        let first = new_page();
        assert!(first > round);
        assert!(STOPPED_ROUND.load(Ordering::Relaxed) >= round, "the old page's work stops");
        let _ = old;
        // The old page went on counting before its last request arrived; a second reload
        // follows before that request comes in.
        let late = first + 5;
        let second = new_page();
        assert!(second > late, "each page gets rounds of its own");
        assert!(STOPPED_ROUND.load(Ordering::Relaxed) >= late, "a late request of an old page stops");
        assert!(STOPPED_ROUND.load(Ordering::Relaxed) < second);
    }

    /// The filmstrip gives way to a waveform of the same editor round, and runs after it.
    #[test]
    fn a_filmstrip_waits_for_the_waveforms() {
        let (_rounds, round) = test_rounds();
        let strip = Demand::yielding(round);
        let wave = Demand::new(round);
        assert!(strip.wanted());
        wait_as_editor(|| {
            assert!(!strip.wanted(), "gives way while the waveform waits");
            assert!(wave.wanted(), "the waveform does not");
            assert!(strip.alive(), "and is still needed afterwards");
        });
        assert!(wait_until_wanted(&strip));
        end_round(round);
        assert!(!wait_until_wanted(&strip), "an ended round needs nothing");
    }

    /// The interleaving that once lost a filmstrip: its job stops while another editor job
    /// waits, and the editor asks for it again in a new round meanwhile.
    #[test]
    fn a_renewed_request_keeps_a_filmstrip_that_is_giving_way() {
        let (_rounds, round) = test_rounds();
        let pending = Pending::default();
        let demand = pending.claim("clip", round, true).expect("a new job");
        end_round(round);
        assert!(!demand.alive(), "the worker sees its round end");
        // Reopened: the request joins the job under way, for the new round.
        assert!(pending.claim("clip", round + 1, true).is_none());
        wait_as_editor(|| {
            assert!(!demand.wanted(), "a waveform waits, so the filmstrip may not run");
            assert!(pending.stopped("clip"), "but it is kept, as the new round needs it");
        });
        assert!(demand.wanted());
        pending.finish("clip");
    }

    /// Losing the focus pauses editor work; it goes on once Replix has the focus again.
    #[test]
    fn editor_work_pauses_without_the_focus_and_goes_on() {
        let (_rounds, round) = test_rounds();
        let demand = Demand::new(round);
        let tries = AtomicUsize::new(0);
        let outcome = std::thread::scope(|s| {
            let job = s.spawn(|| {
                run_needed(&demand, |go| {
                    if tries.fetch_add(1, Ordering::Relaxed) == 0 {
                        crate::FOCUSED.store(false, Ordering::Relaxed);
                        anyhow::ensure!(go(), "paused");
                    }
                    Ok(())
                })
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
            crate::FOCUSED.store(true, Ordering::Relaxed);
            wake();
            job.join().unwrap()
        });
        assert!(matches!(outcome, Outcome::Done(())));
        assert_eq!(tries.load(Ordering::Relaxed), 2, "ran again after the pause");
    }

    #[test]
    fn stopped_work_reports_a_stop_not_a_failure() {
        let (_rounds, round) = test_rounds();
        let demand = Demand::new(round);
        let outcome = run(&demand, |go| {
            end_round(round);
            anyhow::ensure!(go(), "stopped");
            Ok(())
        });
        assert!(matches!(outcome, Outcome::Stopped));
        let demand = Demand::new(round + 1);
        assert!(matches!(run(&demand, |_| anyhow::bail!("broken") as anyhow::Result<()>), Outcome::Failed(_)));
    }
}
