//! Keeps the main thread's run loop going while the app works elsewhere.
//!
//! Apple Translation finishes its async work on the main dispatch queue, which
//! only runs while the main thread spins its CoreFoundation run loop (finding
//! of spike #4 / #10). So `main` hands the whole app (tokio and all) to a second
//! thread and spins the run loop until that thread returns.

use std::ffi::c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopRun();
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopStop(rl: *mut c_void);
}

struct MainLoop(*mut c_void);
// SAFETY: CFRunLoopStop may be called from any thread.
unsafe impl Send for MainLoop {}

/// Runs `work` on a new thread while this thread, which must be the main
/// thread, spins the main run loop. Returns what `work` returns.
pub fn run_with_main_loop<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    // SAFETY: plain CoreFoundation calls with no arguments to validate.
    let main = MainLoop(unsafe { CFRunLoopGetMain() });
    let handle = std::thread::spawn(move || {
        let out = work();
        let main = main;
        // SAFETY: `main` is the process's main run loop, valid for its lifetime.
        // A stop sent before the loop starts is kept and ends the next run.
        unsafe { CFRunLoopStop(main.0) };
        out
    });
    while !handle.is_finished() {
        // SAFETY: called on the main thread, as documented above.
        unsafe { CFRunLoopRun() };
    }
    match handle.join() {
        Ok(out) => out,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
