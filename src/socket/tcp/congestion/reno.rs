use crate::{socket::tcp::RttEstimator, time::Instant};

use super::{Controller, initial_window};

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Reno {
    cwnd: usize,
    min_cwnd: usize,
    ssthresh: usize,
    rwnd: usize,
}

impl Reno {
    pub fn new() -> Self {
        Reno {
            cwnd: initial_window(super::super::DEFAULT_MSS),
            min_cwnd: super::super::DEFAULT_MSS,
            ssthresh: usize::MAX,
            rwnd: 64 * 1024,
        }
    }
}

impl Controller for Reno {
    fn window(&self) -> usize {
        self.cwnd
    }

    fn on_ack(&mut self, _now: Instant, len: usize, _rtt: &RttEstimator) {
        // Only an ACK of new data opens the window (RFC 5681 section 3.1); a duplicate ACK
        // acknowledges nothing.
        if len == 0 {
            return;
        }
        let increase = if self.cwnd < self.ssthresh {
            // Slow start: at most one segment per ACK (RFC 5681 equation 2).
            len.min(self.min_cwnd)
        } else {
            // Congestion avoidance: about one segment per round trip (RFC 5681 equation 3).
            (self.min_cwnd * self.min_cwnd / self.cwnd).max(1)
        };

        self.cwnd = self
            .cwnd
            .saturating_add(increase)
            .min(self.rwnd)
            .max(self.min_cwnd);
    }

    fn on_duplicate_ack(&mut self, _now: Instant) {
        self.ssthresh = (self.cwnd >> 1).max(self.min_cwnd);
    }

    fn on_retransmit(&mut self, _now: Instant) {
        self.cwnd = (self.cwnd >> 1).max(self.min_cwnd);
    }

    fn set_mss(&mut self, mss: usize) {
        self.min_cwnd = mss;
        self.cwnd = initial_window(mss).min(self.rwnd);
    }

    fn set_remote_window(&mut self, remote_window: usize) {
        if self.rwnd < remote_window {
            self.rwnd = remote_window;
        }
    }
}

#[cfg(test)]
mod test {
    use crate::time::Instant;

    use super::*;

    #[test]
    fn test_reno() {
        let remote_window = 64 * 1024;
        let now = Instant::from_millis(0);

        for i in 0..10 {
            for j in 0..9 {
                let mut reno = Reno::new();
                reno.set_mss(1480);

                // Set remote window.
                reno.set_remote_window(remote_window);

                reno.on_ack(now, 4096, &RttEstimator::default());

                let mut n = i;
                for _ in 0..j {
                    n *= i;
                }

                if i & 1 == 0 {
                    reno.on_retransmit(now);
                } else {
                    reno.on_duplicate_ack(now);
                }

                let elapsed = Instant::from_millis(1000);
                reno.on_ack(elapsed, n, &RttEstimator::default());

                let cwnd = reno.window();
                println!("Reno: elapsed = {}, cwnd = {}", elapsed, cwnd);

                assert!(cwnd >= reno.min_cwnd);
                assert!(reno.window() <= remote_window);
            }
        }
    }

    #[test]
    fn reno_congestion_avoidance() {
        let now = Instant::from_millis(0);
        let rtt = RttEstimator::default();
        let mut reno = Reno::new();
        reno.set_mss(100);
        // A duplicate ACK halves the threshold; the window waits for the retransmission.
        reno.on_duplicate_ack(now);
        assert_eq!(reno.ssthresh, 200);
        assert_eq!(reno.window(), 400);
        // Further duplicate ACKs, which acknowledge nothing, neither open the window nor
        // undo the threshold.
        reno.on_ack(now, 0, &rtt);
        assert_eq!(reno.window(), 400);
        assert_eq!(reno.ssthresh, 200);
        // In congestion avoidance a window's worth of ACKs opens it by about one segment.
        for _ in 0..4 {
            reno.on_ack(now, 100, &rtt);
        }
        assert!((480..=500).contains(&reno.window()), "{}", reno.window());
    }

    #[test]
    fn reno_min_cwnd() {
        let remote_window = 64 * 1024;
        let now = Instant::from_millis(0);

        let mut reno = Reno::new();
        reno.set_remote_window(remote_window);

        for _ in 0..100 {
            reno.on_retransmit(now);
            assert!(reno.window() >= reno.min_cwnd);
        }
    }

    #[test]
    fn reno_set_rwnd() {
        let mut reno = Reno::new();
        reno.set_remote_window(64 * 1024 * 1024);

        println!("{reno:?}");
    }
}
