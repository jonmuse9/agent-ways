use std::io::{self, Write};

/// Emit a batch of sensor disclosures to stdout for Monitor delivery.
/// All lines emitted within 200ms are batched by Monitor into one notification.
pub fn emit_batch(disclosures: &[(String, String, String)]) {
    for (sensor, priority, summary) in disclosures {
        println!("[attend sensor={sensor} priority={priority}] {summary}");
    }
    io::stdout().flush().ok();
}

/// Log diagnostic info to stderr (does not become a Monitor notification).
pub fn log(message: &str) {
    eprintln!("[attend] {message}");
}
