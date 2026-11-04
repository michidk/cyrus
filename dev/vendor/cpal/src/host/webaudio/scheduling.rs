// Browser scheduling is kept independent of WebAudio so its clock contract can be tested.
pub(crate) fn next_buffer_start(planned: f64, now: f64) -> f64 {
    planned.max(now + 0.025)
}

#[cfg(test)]
mod tests {
    use super::next_buffer_start;

    #[test]
    fn expired_buffers_resume_sequentially_after_the_current_clock() {
        let duration = 2048.0 / 44100.0;
        let first = next_buffer_start(1.0, 12.0);
        let second = next_buffer_start(first + duration, 12.001);
        assert_eq!(first, 12.025);
        assert_eq!(second, first + duration);
    }

    #[test]
    fn queued_buffers_retain_their_start_and_initial_buffers_get_a_margin() {
        assert_eq!(next_buffer_start(2.0, 1.0), 2.0);
        assert_eq!(next_buffer_start(0.0, 0.0), 0.025);
    }
}
