//! Foreground terminal ownership for a child in its own process group.
//! All signal-mask changes happen in the parent after spawning; child signal masks stay intact.
#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io::{self, IsTerminal};
    use std::os::fd::AsFd;

    use nix::errno::Errno;
    use nix::sys::signal::{SigSet, SigmaskHow, Signal, killpg, pthread_sigmask};
    use nix::unistd::{Pid, getpgrp, tcgetpgrp, tcsetpgrp};

    use crate::ports::process::{OutputPolicy, ProcessCommand, StdinPolicy};

    pub struct ForegroundTerminal {
        terminal: File,
        previous: Pid,
        attached: bool,
    }

    impl ForegroundTerminal {
        pub fn capture(command: &ProcessCommand) -> io::Result<Option<Self>> {
            let descriptor = if command.stdin == StdinPolicy::Inherit && io::stdin().is_terminal() {
                io::stdin().as_fd().try_clone_to_owned()?
            } else if command.stderr == OutputPolicy::Inherit && io::stderr().is_terminal() {
                io::stderr().as_fd().try_clone_to_owned()?
            } else if command.stdout == OutputPolicy::Inherit && io::stdout().is_terminal() {
                io::stdout().as_fd().try_clone_to_owned()?
            } else {
                return Ok(None);
            };
            let terminal = File::from(descriptor);
            let previous = match tcgetpgrp(&terminal) {
                Ok(previous) => previous,
                // A passed PTY need not be this process's controlling terminal.
                Err(Errno::ENOTTY) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            if previous != getpgrp() {
                return Err(io::Error::other(
                    "interactive child requires a foreground invocation",
                ));
            }
            Ok(Some(Self {
                terminal,
                previous,
                attached: false,
            }))
        }

        pub fn attach(&mut self, child: u32) -> io::Result<()> {
            let child = i32::try_from(child)
                .map(Pid::from_raw)
                .map_err(io::Error::other)?;
            set_foreground(&self.terminal, child)?;
            self.attached = true;
            // A quick read before tcsetpgrp may have stopped the child with SIGTTIN.
            match killpg(child, Signal::SIGCONT) {
                Ok(()) | Err(Errno::ESRCH) => Ok(()),
                Err(error) => Err(error.into()),
            }
        }

        pub fn restore(&mut self) -> io::Result<()> {
            if self.attached {
                set_foreground(&self.terminal, self.previous)?;
                self.attached = false;
            }
            Ok(())
        }
    }

    impl Drop for ForegroundTerminal {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    fn set_foreground(terminal: &File, group: Pid) -> io::Result<()> {
        let mut blocked = SigSet::empty();
        blocked.add(Signal::SIGTTOU);
        let mut previous = SigSet::empty();
        pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&blocked), Some(&mut previous))?;
        let result = tcsetpgrp(terminal, group);
        let restored = pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&previous), None);
        result.and(restored).map_err(Into::into)
    }
}

#[cfg(unix)]
pub(super) use unix::ForegroundTerminal;

#[cfg(not(unix))]
pub(super) struct ForegroundTerminal;

#[cfg(not(unix))]
impl ForegroundTerminal {
    pub fn capture(
        _command: &crate::ports::process::ProcessCommand,
    ) -> std::io::Result<Option<Self>> {
        Ok(None)
    }
    pub fn attach(&mut self, _child: u32) -> std::io::Result<()> {
        Ok(())
    }
    pub fn restore(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
