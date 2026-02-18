const SLEEP_DURATION: u64 = 300;

/// After this function was called, the hardware goes into deep sleep, only the RTC clock is kept running, and it's variables are kept.
/// After the duration elapsed, the hardware will wake up and start executing from the beginning of the program.
pub(crate) fn go_to_deep_sleep(rtc: &mut esp_hal::rtc_cntl::Rtc<'_>) -> ! {
    let sleep_time = core::time::Duration::from_secs(SLEEP_DURATION);
    let wake_sources = esp_hal::rtc_cntl::sleep::TimerWakeupSource::new(sleep_time);
    log::info!("Going to sleep for {:?}...", sleep_time);
    rtc.sleep_deep(&[&wake_sources]);
}
