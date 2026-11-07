//! Frontend-only formatting of Session traffic snapshots; never measures or sends traffic.

use mcpm_exec::messaging::SessionTrafficStats;
use ratatui::text::Line;

pub(super) fn lines(stats: Option<SessionTrafficStats>, full: bool) -> Vec<Line<'static>> {
    let Some(stats) = stats else {
        return vec![Line::from("Speed unavailable (no messaging Session)")];
    };
    let sent = stats.sent;
    let received = stats.received;
    if full {
        vec![
            Line::from(format!(
                " 100ms: P {} · AVG {}",
                rates(sent.peak_100ms, received.peak_100ms),
                rates(sent.current_100ms, received.current_100ms),
            )),
            Line::from(format!(
                "1000ms: P {} · AVG {}",
                rates(sent.peak_1s, received.peak_1s),
                rates(sent.current_1s, received.current_1s),
            )),
        ]
    } else {
        vec![Line::from(rates(
            sent.current_100ms,
            received.current_100ms,
        ))]
    }
}

fn rates(sent: f64, received: f64) -> String {
    // KB is decimal; the total P is the sum of the two directional lifetime peaks,
    // not a separately sampled peak of simultaneous bidirectional traffic.
    format!(
        "{:.1}KB/s(↑{:.1}KB/s ↓{:.1}KB/s)",
        (sent + received) / 1000.0,
        sent / 1000.0,
        received / 1000.0,
    )
}
