use core::cell::RefCell;

use critical_section::Mutex;
use esp_hal::{
    gpio::{AnyPin, Input},
    handler, ram,
    rtc_cntl::sleep::{Ext0WakeupSource, TimerWakeupSource, WakeupLevel},
    system::{SleepSource, wakeup_cause},
};

use crate::{BOOT_TYPES, BootType};

const SLEEP_DURATION: u64 = 300;
const TZ: jiff::tz::TimeZone = jiff::tz::TimeZone::fixed(jiff::tz::offset(1));

pub static BUTTON: Mutex<RefCell<Option<Input<'static>>>> = Mutex::new(RefCell::new(None));

pub(crate) fn go_to_deep_sleep(rtc: &mut esp_hal::rtc_cntl::Rtc<'_>) -> ! {
    let sleep_time = core::time::Duration::from_secs(SLEEP_DURATION);
    let timer_wakeup = TimerWakeupSource::new(sleep_time);

    let had_button = critical_section::with(|cs| BUTTON.borrow_ref_mut(cs).take().is_some());

    log::info!("Going to sleep for {:?}...", sleep_time);

    if had_button {
        let pin: AnyPin<'static> = unsafe { AnyPin::steal(0) };
        let ext0 = Ext0WakeupSource::new(pin, WakeupLevel::Low);
        rtc.sleep_deep(&[&timer_wakeup, &ext0]);
    } else {
        rtc.sleep_deep(&[&timer_wakeup]);
    }
}

pub(crate) fn get_time(rtc: &esp_hal::rtc_cntl::Rtc<'_>) -> jiff::Zoned {
    let now = jiff::Timestamp::from_microsecond(rtc.current_time_us() as i64).unwrap();
    now.to_zoned(TZ)
}

// Sets the boot type based on wakeup cause
pub(crate) fn apply_wakeup_boot_type() {
    match wakeup_cause() {
        // GPIO0 button was pressed
        SleepSource::Ext0 => BootType::set(BootType::Config),
        // Timer expired
        SleepSource::Timer => BootType::set(BootType::Display),
        // For other sources keep the current state
        _ => {}
    }
}

#[handler]
#[ram]
pub fn handler() {
    critical_section::with(|cs| {
        BUTTON
            .borrow_ref_mut(cs)
            .as_mut()
            .unwrap()
            .clear_interrupt();
    });
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
    esp_hal::system::software_reset();
}
