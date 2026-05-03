//! Desktop notifications for agent status changes

use crate::session::{Instance, Status};

pub fn send_status_notification(instance: &Instance, old_status: Status, new_status: Status) {
    if old_status == new_status {
        return;
    }

    let msg = match new_status {
        Status::Waiting => format!("🔔 '{}' is waiting for input", instance.title),
        Status::Completed => format!("✅ '{}' completed successfully", instance.title),
        Status::Error => format!("❌ '{}' encountered an error", instance.title),
        _ => return, // Only notify for important status changes
    };

    // For now, print to console/terminal
    // TODO: Integrate with notify-rust for system notifications
    eprintln!("{}", msg);
}