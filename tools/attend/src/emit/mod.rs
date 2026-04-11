use std::io::{self, Write};

/// Emit a batch of sensor disclosures to stdout for Monitor delivery.
/// Each event is its own line — Monitor batches lines within 200ms into
/// one notification, so rapid events group naturally without truncation.
pub fn emit_batch(disclosures: &[(String, String, Vec<String>)]) {
    for (sensor, priority, events) in disclosures {
        for event in events {
            println!("[attend sensor={sensor} priority={priority}] {event}");
        }
    }
    io::stdout().flush().ok();
}

/// Log diagnostic info to stderr (does not become a Monitor notification).
pub fn log(message: &str) {
    eprintln!("[attend] {message}");
}
