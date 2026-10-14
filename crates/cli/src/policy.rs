/// Whether a command must be executed online or is permitted to directly access local state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPolicy {
    OnlineOnly,
    OnlinePreferred,
    OfflineAllowed,
}
