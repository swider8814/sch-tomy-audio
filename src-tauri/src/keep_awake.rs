//! Keeps the computer from going to sleep while files are processed or
//! downloaded (a backlog of long recordings takes hours). Released as soon
//! as the queue is idle. The display may still turn off.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    /// The execution state belongs to the calling thread, so the same
    /// (long-lived) thread must call `set` for both states.
    pub struct Awake;
    impl Awake {
        pub fn new() -> Self {
            Awake
        }
        pub fn set(&mut self, on: bool) {
            let flags = if on {
                ES_CONTINUOUS | ES_SYSTEM_REQUIRED
            } else {
                ES_CONTINUOUS
            };
            // SAFETY: plain Win32 call without pointers.
            unsafe {
                SetThreadExecutionState(flags);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Child, Command, Stdio};

    /// `caffeinate -i -w <pid>`: prevents idle sleep while running and ends
    /// by itself if the app goes away.
    pub struct Awake(Option<Child>);
    impl Awake {
        pub fn new() -> Self {
            Awake(None)
        }
        pub fn set(&mut self, on: bool) {
            if on && self.0.is_none() {
                self.0 = Command::new("/usr/bin/caffeinate")
                    .args(["-i", "-w", &std::process::id().to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok();
            } else if !on {
                if let Some(mut c) = self.0.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    /// Linux is a development platform only: no-op.
    pub struct Awake;
    impl Awake {
        pub fn new() -> Self {
            Awake
        }
        pub fn set(&mut self, _on: bool) {}
    }
}

pub use imp::Awake;
