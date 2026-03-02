// Desktop notifications — send OS-native notifications on operation completion.
//
// Uses notify-rust to show toast notifications on Linux (via D-Bus/libnotify),
// macOS (via osascript/NSUserNotification), and Windows (via WinRT toast).

#![allow(dead_code)]

use log::{info, warn};

/// Notification urgency/type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    /// Operation completed successfully.
    Success,
    /// Operation failed with an error.
    Failure,
    /// Informational notice (e.g., download complete, ready to write).
    Info,
}

/// Send a desktop notification.
///
/// If the notification subsystem is unavailable (e.g., headless server, no D-Bus),
/// this logs a warning and returns Ok(()) — it never fails the operation.
pub fn send_notification(kind: NotifyKind, title: &str, body: &str) {
    info!("Notification [{}]: {} — {}", kind_label(kind), title, body);

    let result = notify_rust::Notification::new()
        .appname("AgenticBlockTransfer")
        .summary(title)
        .body(body)
        .icon(icon_for_kind(kind))
        .timeout(timeout_for_kind(kind))
        .show();

    match result {
        Ok(_) => {
            info!("Desktop notification sent successfully");
        }
        Err(e) => {
            warn!("Could not send desktop notification: {}. This is non-fatal.", e);
        }
    }
}

/// Convenience: notify that a write operation completed.
pub fn notify_write_complete(target: &str, bytes_written: u64, elapsed_secs: f64) {
    let size = humansize::format_size(bytes_written, humansize::BINARY);
    let body = format!(
        "Wrote {} to {} in {:.1}s",
        size, target, elapsed_secs
    );
    send_notification(NotifyKind::Success, "Write Complete", &body);
}

/// Convenience: notify that a write operation failed.
pub fn notify_write_failed(target: &str, error: &str) {
    let body = format!("Failed writing to {}: {}", target, error);
    send_notification(NotifyKind::Failure, "Write Failed", &body);
}

/// Convenience: notify that verification succeeded.
pub fn notify_verify_complete(target: &str) {
    let body = format!("Verification of {} passed — data integrity confirmed", target);
    send_notification(NotifyKind::Success, "Verification Passed", &body);
}

/// Convenience: notify that verification failed.
pub fn notify_verify_failed(target: &str) {
    let body = format!("Verification of {} FAILED — data may be corrupt", target);
    send_notification(NotifyKind::Failure, "Verification Failed", &body);
}

fn kind_label(kind: NotifyKind) -> &'static str {
    match kind {
        NotifyKind::Success => "SUCCESS",
        NotifyKind::Failure => "FAILURE",
        NotifyKind::Info => "INFO",
    }
}

fn icon_for_kind(kind: NotifyKind) -> &'static str {
    match kind {
        NotifyKind::Success => "dialog-information",
        NotifyKind::Failure => "dialog-error",
        NotifyKind::Info => "dialog-information",
    }
}

fn timeout_for_kind(kind: NotifyKind) -> notify_rust::Timeout {
    match kind {
        NotifyKind::Success => notify_rust::Timeout::Milliseconds(5000),
        NotifyKind::Failure => notify_rust::Timeout::Milliseconds(10000),
        NotifyKind::Info => notify_rust::Timeout::Milliseconds(3000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_labels() {
        assert_eq!(kind_label(NotifyKind::Success), "SUCCESS");
        assert_eq!(kind_label(NotifyKind::Failure), "FAILURE");
        assert_eq!(kind_label(NotifyKind::Info), "INFO");
    }

    #[test]
    fn icons_are_valid() {
        // Just ensure they return non-empty strings
        assert!(!icon_for_kind(NotifyKind::Success).is_empty());
        assert!(!icon_for_kind(NotifyKind::Failure).is_empty());
    }
}
