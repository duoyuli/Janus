//! How many agent processes Janus keeps open at once. Every ACP session's adapter and agent take a
//! seat before they start, across goal runs, chats and Loops, and give it back when the process
//! goes (killed or dropped). When every seat is taken, who asks waits, and seats go first to what
//! the person is waiting for, then to what a schedule starts, then to a team's collaborators.
// @project-doc docs/domains/20261031-run_lifecycle.md#agent_seats

use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Who asks for an agent, in the order seats are handed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Asker {
    /// The person is waiting: a message in a chat or after delivery, a Loop woken by hand or by an
    /// answer, a run continued by hand.
    Person,
    /// A schedule starts it: a goal taken off the dispatch queue, a Loop's timer.
    Schedule,
    /// A collaborator's attempt in a run already working.
    Team,
}

pub struct Seats {
    cap: usize,
    st: Mutex<State>,
}

#[derive(Default)]
struct State {
    open: usize,
    /// Who waits for a seat, in the order they asked.
    waiting: Vec<(Asker, u64, oneshot::Sender<Seat>)>,
    asked: u64,
}

/// One open agent process's place. Dropping it gives the place back: to the first in line, or free.
pub struct Seat {
    seats: Arc<Seats>,
}

impl Seats {
    pub fn new(cap: usize) -> Arc<Seats> {
        Arc::new(Seats { cap: cap.max(1), st: Mutex::default() })
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Seats taken now.
    #[cfg(test)]
    pub fn open(&self) -> usize {
        self.st.lock().unwrap().open
    }

    /// A seat now if one is free, else a place in line: the seat arrives when one is given back
    /// and nobody before this one (by `Asker`, then by order) waits.
    pub fn take(self: &Arc<Self>, asker: Asker) -> Result<Seat, oneshot::Receiver<Seat>> {
        let mut st = self.st.lock().unwrap();
        if st.open < self.cap {
            st.open += 1;
            return Ok(Seat { seats: self.clone() });
        }
        let (tx, rx) = oneshot::channel();
        st.asked += 1;
        let n = st.asked;
        st.waiting.push((asker, n, tx));
        Err(rx)
    }

    fn give_back(self: &Arc<Self>) {
        let mut st = self.st.lock().unwrap();
        loop {
            let Some(i) = (0..st.waiting.len()).min_by_key(|&i| (st.waiting[i].0, st.waiting[i].1)) else {
                st.open -= 1;
                return;
            };
            let (_, _, tx) = st.waiting.remove(i);
            // The seat moves on without being counted again; one whose taker stopped waiting is
            // passed to the next in line instead (forgotten, so it is not given back twice).
            match tx.send(Seat { seats: self.clone() }) {
                Ok(()) => return,
                Err(seat) => std::mem::forget(seat),
            }
        }
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let seats = self.seats.clone();
        seats.give_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seats_go_to_the_person_first_and_never_twice() {
        let seats = Seats::new(2);
        let a = seats.take(Asker::Team).ok().unwrap();
        let b = seats.take(Asker::Team).ok().unwrap();
        assert_eq!(seats.open(), 2);
        let mut team = seats.take(Asker::Team).err().unwrap();
        let mut schedule = seats.take(Asker::Schedule).err().unwrap();
        let gone = seats.take(Asker::Person).err().unwrap();
        let mut person = seats.take(Asker::Person).err().unwrap();
        // One who stopped waiting is skipped.
        drop(gone);
        drop(a);
        let p = person.try_recv().expect("the person first");
        assert!(schedule.try_recv().is_err() && team.try_recv().is_err());
        drop(b);
        let s = schedule.try_recv().expect("then a schedule");
        drop(p);
        let t = team.try_recv().expect("then the team");
        assert_eq!(seats.open(), 2, "seats handed on are not counted again");
        drop(s);
        drop(t);
        assert_eq!(seats.open(), 0);
        let _again = seats.take(Asker::Team).ok().expect("free again");
    }
}
