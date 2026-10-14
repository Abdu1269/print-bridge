#[cfg(unix)]
use std::os::unix::net::UnixDatagram;

/// Send READY/STOPPING status to systemd; no-op in non-systemd environments.
pub fn notify(message: &str) -> std::io::Result<()> {
    let Some(socket) = std::env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        let socket = std::path::PathBuf::from(socket);
        let datagram = UnixDatagram::unbound()?;
        datagram.send_to(message.as_bytes(), socket)?;
    }
    #[cfg(not(unix))]
    let _ = (socket, message);
    Ok(())
}
