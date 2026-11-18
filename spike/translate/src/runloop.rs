// ponytail: raw CoreFoundation FFI, three symbols — no crate needed for a throwaway spike.
// Translation.framework delivers results on the main queue, so a CLI must keep the
// main run loop alive while the work runs on another thread (finding from ticket #4).
use std::ffi::c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopRun();
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopStop(rl: *mut c_void);
}

struct MainLoop(*mut c_void);
unsafe impl Send for MainLoop {}

/// Run `work` on a worker thread while the main thread spins the run loop.
pub fn run_with_main_loop<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let main = MainLoop(unsafe { CFRunLoopGetMain() });
    let handle = std::thread::spawn(move || {
        let out = work();
        let main = main;
        unsafe { CFRunLoopStop(main.0) };
        out
    });
    while !handle.is_finished() {
        unsafe { CFRunLoopRun() };
    }
    handle.join().expect("worker panicked")
}
