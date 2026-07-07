use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

pub fn terminates<F>(duration_ms: u64, f: F) -> bool
where
    F: Send + FnOnce() + 'static,
{
    terminates_async(duration_ms, f, || {})
}

pub fn terminates_async<F, G>(duration_ms: u64, f: F, g: G) -> bool
where
    F: Send + FnOnce() + 'static,
    G: FnOnce(),
{
    async_test(duration_ms, f, g).is_some()
}

pub fn async_test<T, F, G>(duration_ms: u64, f: F, g: G) -> Option<T>
where
    F: Send + FnOnce() -> T + 'static,
    G: FnOnce(),
    T: Send + 'static,
{
    let (tx, rx) = channel();

    thread::spawn(move || {
        let t = f();
        let _ = tx.send(t);
    });

    g();

    if let a @ Some(_) = rx.try_recv().ok() {
        return a;
    }

    for _ in 0..duration_ms / 50 {
        thread::sleep(Duration::from_millis(50));
        if let a @ Some(_) = rx.try_recv().ok() {
            return a;
        }
    }

    thread::sleep(Duration::from_millis(duration_ms % 50));

    rx.try_recv().ok()
}
