use esp_hal::{
    gpio::{AnyPin, Input},
    rtc_cntl::sleep::{Ext0WakeupSource, TimerWakeupSource, WakeupLevel},
    system::{SleepSource, wakeup_cause},
};

use crate::{BOOT_TYPES, BootType};

const SLEEP_DURATION: u64 = 300;
const TZ: jiff::tz::TimeZone = jiff::tz::TimeZone::fixed(jiff::tz::offset(2));

pub(crate) fn go_to_deep_sleep(rtc: &mut esp_hal::rtc_cntl::Rtc<'_>) -> ! {
    let sleep_time = core::time::Duration::from_secs(SLEEP_DURATION);
    let timer_wakeup = TimerWakeupSource::new(sleep_time);

    #[cfg(debug_assertions)]
    {
        let now = jiff::Timestamp::from_microsecond(rtc.current_time_us() as i64);
        if let Ok(now) = now {
            let until = now
                .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(1)))
                .checked_add(sleep_time)
                .unwrap();
            crate::defmt::info!(
                "Going to sleep for {} minutes, until: {:?}",
                sleep_time.as_secs() / 60,
                crate::defmt::Display2Format(&until)
            );
        }
        embassy_time::block_for(embassy_time::Duration::from_millis(100));
    }

    let pin: AnyPin<'static> = unsafe { AnyPin::steal(0) };
    let ext0 = Ext0WakeupSource::new(pin, WakeupLevel::Low);
    rtc.sleep_deep(&[&timer_wakeup, &ext0]);
}

pub(crate) fn get_time(rtc: &esp_hal::rtc_cntl::Rtc<'_>) -> jiff::Zoned {
    let now = jiff::Timestamp::from_microsecond(rtc.current_time_us() as i64).unwrap();
    now.to_zoned(TZ)
}

// Sets the boot type based on wakeup cause
pub(crate) fn apply_wakeup_boot_type() {
    match wakeup_cause() {
        // GPIO0 button was pressed
        SleepSource::Ext0 => {
            crate::defmt::info!("Woke up from GPIO0, setting boot type to Config");
            BootType::set(BootType::Config);
        }
        // Timer expired
        SleepSource::Timer => {
            crate::defmt::info!("Woke up from timer, setting boot type to Display");
            BootType::set(BootType::Display);
        }
        // For other sources keep the current state
        _ => {}
    }
}

#[embassy_executor::task]
pub async fn button_task(mut button: Input<'static>) {
    button.wait_for_falling_edge().await;
    BOOT_TYPES
        .fetch_update(
            core::sync::atomic::Ordering::Relaxed,
            core::sync::atomic::Ordering::Relaxed,
            |x| {
                Some(match BootType::from_u8(x) {
                    BootType::Display => BootType::Config as u8,
                    BootType::Config => BootType::Display as u8,
                })
            },
        )
        .unwrap();
    crate::wifi::stop_wifi_and_reset().await;
}
