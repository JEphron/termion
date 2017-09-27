#[cfg(not(windows))]
use std::{fs, io};

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Is this stream an TTY?
#[cfg(unix)]
pub fn is_tty<T: AsRawFd>(stream: &T) -> bool {
    use libc;

    unsafe { libc::isatty(stream.as_raw_fd()) == 1 }
}

/// This will panic.
#[cfg(target_os = "redox")]
pub fn is_tty<T: AsRawFd>(_stream: &T) -> bool {
    unimplemented!();
}

/// Get the TTY device.
///
/// This allows for getting stdio representing _only_ the TTY, and not other streams.
#[cfg(target_os = "redox")]
pub fn get_tty() -> io::Result<Box<io::Read>> {
    use std::env;
    let tty = try!(env::var("TTY").map_err(|x| io::Error::new(io::ErrorKind::NotFound, x)));
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(tty)
        .map(|file| Box::new(file) as Box<io::Read>)
}

/// Get the TTY device.
///
/// This allows for getting stdio representing _only_ the TTY, and not other streams.
#[cfg(unix)]
pub fn get_tty() -> io::Result<Box<io::Read>> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map(|file| Box::new(file) as Box<io::Read>)
}

#[cfg(not(windows))]
pub fn init() -> Guard {
    Guard
}

#[cfg(not(windows))]
pub struct Guard {}

#[cfg(windows)]
pub struct Guard {
    _pre_init_state: windows::PreInitState
}

#[cfg(windows)]
// it'll be an api-breaking change to do it later
pub /*(crate)*/ mod windows {
    use std::io;
    use std::os::windows::prelude::*;
    use kernel32::{GetStdHandle, GetConsoleMode, SetConsoleMode};
    use winapi::wincon::{ENABLE_PROCESSED_OUTPUT, ENABLE_WRAP_AT_EOL_OUTPUT, ENABLE_LINE_INPUT,
                         ENABLE_PROCESSED_INPUT, ENABLE_ECHO_INPUT};

    use winapi::winbase::{STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    use winapi::{FALSE, DWORD, HANDLE, INVALID_HANDLE_VALUE};

    // once winapi 0.3 is available
    // use winapi::wincon::{ENABLE_VIRTUAL_TERMINAL_PROCESSING};
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: DWORD = 0x0004;

    pub struct PreInitState {
        do_cleanup: bool,
        current_out_mode: DWORD,
        current_in_mode: DWORD,
    }

    impl Drop for PreInitState {
        fn drop(&mut self) {
            if self.do_cleanup {
                println!("cleaning up");
                set_console_mode(StdStream::OUT, self.current_out_mode).ok();
                set_console_mode(StdStream::IN, self.current_in_mode).ok();
            }
        }
    }
    use tty::Guard;
    pub fn init() -> Guard {
        Guard {
            _pre_init_state: do_init().unwrap_or(PreInitState {
                do_cleanup: false,
                current_out_mode: 0,
                current_in_mode: 0,
            })
        }
    }

    fn do_init() -> Result<PreInitState, io::Error> {
        // there are many other console hosts on windows that might actually do something
        // rational with the output escape codes, so if the setup fails, carry on rather
        // than reporting an error. The assumption is that the cleanup in the drop trait
        // will always be able to set the flags that are currently set.
        let current_out_mode = get_console_mode(StdStream::OUT)?;
        let current_in_mode = get_console_mode(StdStream::IN)?;

        let new_out_mode = current_out_mode | ENABLE_PROCESSED_OUTPUT | ENABLE_WRAP_AT_EOL_OUTPUT |
            ENABLE_VIRTUAL_TERMINAL_PROCESSING;

        // ignore failure here and hope we are in a capable third party console
        set_console_mode(StdStream::OUT, new_out_mode).ok();

        // TODO: it seems like ENABLE_VIRTUAL_TERMINAL_INPUT causes ^C to be passed
        // through in the input stream, overiding ENABLE_PROCESSED_INPUT.
        // ENABLE_VIRTUAL_TERMINAL_INPUT is only used for mouse event handling at this
        // point. I'm not sure what the desired behaviour is but if that is not the same
        // maybe it would be simpler
        // to start a thread and wait for the mouse events using the windows console
        // api and post them back in a similar fashion to the async reader.

        let new_in_mode = current_in_mode | ENABLE_PROCESSED_INPUT;
        let new_in_mode = new_in_mode & !ENABLE_ECHO_INPUT;

        // ignore failure here and hope we are in a capable third party console
        set_console_mode(StdStream::IN, new_in_mode).ok();

        println!("cim {:x}, com {:x}", current_in_mode, current_out_mode);

        Ok(PreInitState {
            do_cleanup: true,
            current_out_mode,
            current_in_mode,
        })
    }

    #[derive(Copy, Clone)]
    pub enum StdStream {
        IN,
        OUT,
    }

    pub fn get_std_handle(strm: StdStream) -> io::Result<HANDLE> {
        let which_handle = match strm {
            StdStream::IN => STD_INPUT_HANDLE,
            StdStream::OUT => STD_OUTPUT_HANDLE,
        };

        unsafe {
            match GetStdHandle(which_handle) {
                x if x != INVALID_HANDLE_VALUE => Ok(x),
                _ => Err(io::Error::last_os_error()),
            }
        }
    }

    pub fn set_console_mode(strm: StdStream, new_mode: DWORD) -> io::Result<DWORD> {
        let prev = get_console_mode(strm)?;
        unsafe {
            let handle = get_std_handle(strm)?;
            if SetConsoleMode(handle, new_mode) == FALSE {
                Err(io::Error::last_os_error())
            } else {
                Ok(prev)
            }
        }
    }

    pub fn get_console_mode(strm: StdStream) -> io::Result<DWORD> {
        unsafe {
            let handle = get_std_handle(strm)?;
            let mut mode: DWORD = 0;
            if GetConsoleMode(handle, &mut mode) == FALSE {
                Err(io::Error::last_os_error())
            } else {
                Ok(mode)
            }
        }
    }

    pub fn set_raw_input_mode(enable: bool) -> bool {
        get_console_mode(StdStream::IN)
            .map(|current_mode| {
                let new_mode = if enable {
                    current_mode & !ENABLE_LINE_INPUT
                } else {
                    current_mode | ENABLE_LINE_INPUT
                };
                set_console_mode(StdStream::IN, new_mode)
            })
            .is_ok()
    }

    // TODO: provide an implementation of this, perhaps just delegating to the atty crate?
    pub fn is_tty(_: &AsRawHandle) -> bool {
        true
    }

    /// Get the TTY device.
    ///
    /// This allows for getting stdio representing _only_ the TTY, and not other streams.
    #[cfg(target_os = "windows")]
    pub fn get_tty() -> io::Result<Box<io::Read>> {
        // TODO:
        // should this be CreateFile CONOUT$ ??

        // alternatively, return stdin if is_tty(stdin) else Err() ??

        // use std::env;
        // let tty = try!(env::var("TTY").map_err(|x| io::Error::new(io::ErrorKind::NotFound, x)));
        // fs::OpenOptions::new().read(true).write(true).open(tty)

        Ok(Box::new(io::stdin()))
    }
}

#[cfg(windows)]
pub use self::windows::{init, is_tty, get_tty, set_raw_input_mode, PreInitState};

