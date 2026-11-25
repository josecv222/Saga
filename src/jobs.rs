//! Trabajo en segundo plano: nada pesado pasa por el hilo que dibuja.
use crate::app::App;
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub type Apply = Box<dyn FnOnce(&mut App) + Send>;
type Job = Box<dyn FnOnce() -> Option<Apply> + Send>;
pub type Waker = Arc<dyn Fn() + Send + Sync>;

struct Pool {
    q: Mutex<[VecDeque<Job>; 3]>,
    cv: Condvar,
}
#[derive(Clone)]
pub struct Jobs {
    cpu: Arc<Pool>,
    net: Arc<Pool>,
    tx: crossbeam_channel::Sender<Apply>,
    rx: crossbeam_channel::Receiver<Apply>,
    pending: Arc<AtomicUsize>,
    wake: Waker,
}
impl Jobs {
    pub fn new(wake: Waker) -> Jobs {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mk = || Arc::new(Pool { q: Mutex::new(Default::default()), cv: Condvar::new() });
        let j = Jobs { cpu: mk(), net: mk(), tx, rx, pending: Arc::new(AtomicUsize::new(0)), wake };
        let n = std::thread::available_parallelism().map_or(4, |n| n.get()).div_ceil(2).clamp(2, 6);
        for _ in 0..n {
            j.spawn(j.cpu.clone());
        }
        for _ in 0..4 {
            j.spawn(j.net.clone());
        }
        j
    }
    fn spawn(&self, p: Arc<Pool>) {
        let j = self.clone();
        std::thread::spawn(move || loop {
            let job = {
                let mut q = p.q.lock();
                loop {
                    if let Some(x) = q.iter_mut().find_map(|q| q.pop_front()) {
                        break x;
                    }
                    p.cv.wait(&mut q);
                }
            };
            // un libro dañado no tumba el taller
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or(None);
            if let Some(a) = r {
                let _ = j.tx.send(a);
            }
            j.pending.fetch_sub(1, Ordering::SeqCst);
            (j.wake)();
        });
    }
    fn push(&self, p: &Pool, prio: usize, f: Job) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        p.q.lock()[prio].push_back(f);
        p.cv.notify_one();
    }
    /// Prioridad 0 (ya mismo) a 2 (cuando se pueda).
    pub fn run(&self, prio: usize, f: impl FnOnce() -> Option<Apply> + Send + 'static) {
        self.push(&self.cpu, prio, Box::new(f));
    }
    pub fn net(&self, prio: usize, f: impl FnOnce() -> Option<Apply> + Send + 'static) {
        self.push(&self.net, prio, Box::new(f));
    }
    /// Hilo aparte, para diálogos y otras esperas largas.
    pub fn thread(&self, f: impl FnOnce() -> Option<Apply> + Send + 'static) {
        let j = self.clone();
        std::thread::spawn(move || {
            if let Some(a) = f() {
                let _ = j.tx.send(a);
                (j.wake)();
            }
        });
    }
    pub fn send(&self, a: Apply) {
        let _ = self.tx.send(a);
        (self.wake)();
    }
    pub fn next(&self) -> Option<Apply> {
        self.rx.try_recv().ok()
    }
    pub fn idle(&self) -> bool {
        self.pending.load(Ordering::SeqCst) == 0 && self.rx.is_empty()
    }
}
