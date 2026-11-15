use crate::time::Instant;

use super::RttEstimator;

pub(super) mod no_control;

#[cfg(feature = "socket-tcp-cubic")]
pub(super) mod cubic;

#[cfg(feature = "socket-tcp-reno")]
pub(super) mod reno;

/// The initial congestion window for a sender's maximum segment size `mss` (RFC 5681 section
/// 3.1): four segments up to 1095 octets, three up to 2190, and two above.
#[allow(unused)] // unused without a congestion controller
pub(super) fn initial_window(mss: usize) -> usize {
    match mss {
        0..=1095 => 4 * mss,
        1096..=2190 => 3 * mss,
        _ => 2 * mss,
    }
}

#[allow(unused_variables)]
pub(super) trait Controller {
    /// Returns the number of bytes that can be sent.
    fn window(&self) -> usize;

    /// Set the remote window size.
    fn set_remote_window(&mut self, remote_window: usize) {}

    fn on_ack(&mut self, now: Instant, len: usize, rtt: &RttEstimator) {}

    fn on_retransmit(&mut self, now: Instant) {}

    fn on_duplicate_ack(&mut self, now: Instant) {}

    /// An ACK echoed a CE mark (RFC 3168 section 6.1.2): reduce the window as for a loss.
    fn on_ecn(&mut self, now: Instant) {}

    fn pre_transmit(&mut self, now: Instant) {}

    fn post_transmit(&mut self, now: Instant, len: usize) {}

    /// Set the maximum segment size, when a connection is set up; the window starts over at
    /// [`initial_window`] for it.
    fn set_mss(&mut self, mss: usize) {}
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(super) enum AnyController {
    None(no_control::NoControl),

    #[cfg(feature = "socket-tcp-reno")]
    Reno(reno::Reno),

    #[cfg(feature = "socket-tcp-cubic")]
    Cubic(cubic::Cubic),
}

impl AnyController {
    /// Create a new congestion controller.
    /// `AnyController::new()` selects the best congestion controller based on the features.
    ///
    /// - If `socket-tcp-cubic` feature is enabled, it will use `Cubic`.
    /// - If `socket-tcp-reno` feature is enabled, it will use `Reno`.
    /// - If both `socket-tcp-cubic` and `socket-tcp-reno` features are enabled, it will use `Cubic`.
    ///    - `Cubic` is more efficient regarding throughput.
    ///    - `Reno` is more conservative and is suitable for low-power devices.
    /// - If no congestion controller is available, it will use `NoControl`.
    ///
    /// Users can also select a congestion controller manually by [`super::Socket::set_congestion_control()`]
    /// method at run-time.
    #[allow(unreachable_code)]
    #[inline]
    pub fn new() -> Self {
        #[cfg(feature = "socket-tcp-cubic")]
        {
            return AnyController::Cubic(cubic::Cubic::new());
        }

        #[cfg(feature = "socket-tcp-reno")]
        {
            return AnyController::Reno(reno::Reno::new());
        }

        AnyController::None(no_control::NoControl)
    }

    #[inline]
    pub fn inner_mut(&mut self) -> &mut dyn Controller {
        match self {
            AnyController::None(n) => n,

            #[cfg(feature = "socket-tcp-reno")]
            AnyController::Reno(r) => r,

            #[cfg(feature = "socket-tcp-cubic")]
            AnyController::Cubic(c) => c,
        }
    }

    #[inline]
    pub fn inner(&self) -> &dyn Controller {
        match self {
            AnyController::None(n) => n,

            #[cfg(feature = "socket-tcp-reno")]
            AnyController::Reno(r) => r,

            #[cfg(feature = "socket-tcp-cubic")]
            AnyController::Cubic(c) => c,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_initial_window() {
        // RFC 5681 section 3.1.
        assert_eq!(initial_window(40), 160);
        assert_eq!(initial_window(536), 2144);
        assert_eq!(initial_window(1095), 4380);
        assert_eq!(initial_window(1460), 4380);
        assert_eq!(initial_window(2190), 6570);
        assert_eq!(initial_window(4000), 8000);
    }

    #[test]
    #[cfg(feature = "socket-tcp-reno")]
    fn test_reno_starts_at_initial_window() {
        let mut reno = reno::Reno::new();
        assert_eq!(reno.window(), 2144);
        reno.set_mss(40);
        assert_eq!(reno.window(), 160);
    }

    #[test]
    #[cfg(feature = "socket-tcp-cubic")]
    fn test_cubic_starts_at_initial_window() {
        let mut cubic = cubic::Cubic::new();
        assert_eq!(cubic.window(), 2144);
        cubic.set_mss(40);
        assert_eq!(cubic.window(), 160);
    }
}
