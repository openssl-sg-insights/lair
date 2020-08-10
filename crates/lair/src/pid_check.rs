use crate::*;
use std::{
    io::{Read, Write},
    str::FromStr,
};
use sysinfo::SystemExt;

/// Result from invoking `pid_check()` function.
pub struct PidCheckResult {
    /// Access to the lair store file.
    pub store_file: tokio::fs::File,
}

/// Execute lair pid_check verifying we are the one true Lair process
/// with access to given store / pidfile.
/// This is sync instead of async as it is intended to be used at
/// lair process startup, before we agree to acquire access to the store file.
pub fn pid_check(config: &Config) -> LairResult<PidCheckResult> {
    let mut sys = sysinfo::System::new();

    let mut last_err = None;

    // three time pidfile check loop
    for i in 0..3 {
        if i != 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        match pid_check_write(&config, &mut sys) {
            Ok(_) => {
                last_err = None;
                break;
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    if let Some(e) = last_err {
        return Err(e);
    }

    let mut store_file = std::fs::OpenOptions::new();
    let store_file = store_file
        .append(true)
        .read(true)
        .create(true)
        .open(config.get_store_path())
        .map_err(LairError::other)?;

    Ok(PidCheckResult {
        store_file: tokio::fs::File::from_std(store_file),
    })
}

/// only returns success if we were able to write pidfile with our pid
fn pid_check_write(
    config: &Config,
    sys: &mut sysinfo::System,
) -> LairResult<()> {
    std::fs::create_dir_all(config.get_root_path())
        .map_err(LairError::other)?;

    {
        let mut read_pid = std::fs::OpenOptions::new();
        read_pid.read(true);
        let mut buf = Vec::new();

        match read_pid.open(config.get_pid_path()) {
            Ok(mut read_pid) => {
                read_pid.read_to_end(&mut buf).map_err(LairError::other)?;
                let pid =
                    sysinfo::Pid::from_str(&String::from_utf8_lossy(&buf))
                        .map_err(LairError::other)?;
                sys.refresh_process(pid);
                if sys.get_process(pid).is_some() {
                    // a lair process is already running-abort running this one
                    // note - after a system restart the pid may have been
                    // reused perhaps we should check the unix socket for a
                    // valid lair process on the other end??
                    return Err(LairError::ProcessAlreadyExists);
                } else {
                    // there was not a process running under this pid
                    // we can remove it as stale.
                    std::fs::remove_file(config.get_pid_path())
                        .map_err(LairError::other)?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // ok to proceed, as there shouldn't be a process running
            }
            Err(e) => return Err(LairError::other(e)),
        }
    }

    let mut write_pid = std::fs::OpenOptions::new();
    let mut write_pid = write_pid
        .write(true)
        .create_new(true)
        .open(config.get_pid_path())
        .map_err(LairError::other)?;

    write_pid
        .write_all(format!("{}", sysinfo::get_current_pid()?).as_bytes())
        .map_err(LairError::other)?;

    Ok(())
}
