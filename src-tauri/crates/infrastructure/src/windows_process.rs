//! Platform-specific configuration for background child processes.

/// Prevent short-lived background commands from opening console windows.
///
/// Android device and VPN status polling invokes ADB frequently. A Windows GUI process must set
/// `CREATE_NO_WINDOW` on each child process or every poll can briefly flash a console window.
#[cfg(windows)]
pub fn configure_background_process(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn configure_background_process(_: &mut std::process::Command) {}
